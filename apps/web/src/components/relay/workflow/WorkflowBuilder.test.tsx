/**
 * WorkflowBuilder interaction tests (Phase: dead-UI regression guard).
 *
 * Guards the "click does nothing" bug class: the toolbar Run test button is
 * wired to a REAL panel + REAL `POST /workflows/:id/run` mutation, and the
 * panel actually renders. A stubbed `RunPanel` (return null) or an unwired
 * `onRun` makes these tests red.
 */

import { beforeEach, describe, expect, test } from "bun:test";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createRouter, RouterProvider, createRootRoute, createRoute } from "@tanstack/react-router";
import { render, screen, fireEvent, act, waitFor } from "@testing-library/react";
import { WorkflowBuilder } from "./WorkflowBuilder";

const WORKFLOW_ID = "production-gateway";

const latestVersion = {
    id: "v1",
    workflow_id: WORKFLOW_ID,
    version: 3,
    plan_hash: "plan_abc123",
    status: "active",
    created_at: "2026-09-14T00:00:00Z",
    workflow_json: {
        schema_version: 2,
        id: WORKFLOW_ID,
        name: "Production Gateway",
        version: 3,
        nodes: [
            {
                id: "input",
                kind: "input",
                config: { kind: "input" },
                inputs: [],
                outputs: [{ name: "out", port_type: "message" }],
                position: { x: 0, y: 0 },
            },
            {
                id: "output",
                kind: "output",
                config: { kind: "output" },
                inputs: [{ name: "in", port_type: "message" }],
                outputs: [],
                position: { x: 280, y: 0 },
            },
        ],
        edges: [
            {
                id: "e1",
                source_node: "input",
                source_port: "out",
                target_node: "output",
                target_port: "in",
            },
        ],
    },
};

/** Minimal TanStack router so the editor's <Link> renders in tests. */
function makeRouter() {
    const rootRoute = createRootRoute();
    const editorRoute = createRoute({
        getParentRoute: () => rootRoute,
        path: "/workflows/$workflowId",
        component: () => <WorkflowBuilder workflowId={WORKFLOW_ID} />,
    });
    const versionsRoute = createRoute({
        getParentRoute: () => rootRoute,
        path: "/workflows/$workflowId/versions",
        component: () => null,
    });
    const tree = rootRoute.addChildren([editorRoute, versionsRoute]);
    return createRouter({ routeTree: tree, defaultPreload: false });
}

function renderBuilder() {
    const client = new QueryClient({
        defaultOptions: { queries: { retry: false } },
    });
    const router = makeRouter();
    router.history.push(`/workflows/${WORKFLOW_ID}`);
    render(
        <QueryClientProvider client={client}>
            <RouterProvider router={router} />
        </QueryClientProvider>,
    );
    return { client, router };
}

function stubFetch(
    routes: Array<{
        test: (url: string) => boolean;
        body?: unknown;
        sseBody?: unknown;
        status?: number;
        onCall?: (init?: RequestInit) => void;
    }>,
) {
    globalThis.fetch = ((input: RequestInfo | URL, init?: RequestInit) => {
        const url = String(input);
        for (const r of routes) {
            if (r.test(url)) {
                r.onCall?.(init);
                const status = r.status ?? 200;
                if (status >= 400) {
                    return Promise.resolve(
                        new Response(JSON.stringify(r.body), {
                            status,
                            headers: { "content-type": "application/json" },
                        }),
                    );
                }
                // SSE stream URL: return real Response with SSE wire format
                // so runWorkflowStream can read from resp.body.getReader().
                if (r.sseBody) {
                    const sseData = `event: done\ndata: ${JSON.stringify(r.sseBody)}\n\n`;
                    return Promise.resolve(
                        new Response(sseData, {
                            status: 200,
                            headers: { "content-type": "text/event-stream" },
                        }),
                    );
                }
                return Promise.resolve(
                    new Response(JSON.stringify(r.body), {
                        status,
                        headers: { "content-type": "application/json" },
                    }),
                );
            }
        }
        return Promise.reject(new Error(`unexpected fetch: ${url}`));
    }) as unknown as typeof fetch;
}

beforeEach(() => {
    document.body.innerHTML = "";
});

const click = async (el: HTMLElement) => {
    await act(async () => {
        fireEvent.click(el);
    });
};

describe("WorkflowBuilder Run test", () => {
    test("toolbar Run test opens the real panel (not a no-op / null render)", async () => {
        stubFetch([
            {
                test: (u) => u.includes(`/workflows/${WORKFLOW_ID}/versions`),
                body: [latestVersion],
            },
            { test: (u) => u.includes("/lanes?"), body: [] },
            { test: (u) => u.includes("/providers"), body: [] },
        ]);
        renderBuilder();

        await screen.findByText(WORKFLOW_ID, {}, { timeout: 2000 });

        await click(screen.getByRole("button", { name: /Run test/i }));

        expect(screen.getByText("Request body")).toBeTruthy();
        expect(screen.getByRole("button", { name: /^Run$/i })).toBeTruthy();
    });

    test("submitting the run renders the REAL backend envelope + output", async () => {
        stubFetch([
            {
                test: (u) => u.includes(`/workflows/${WORKFLOW_ID}/versions`),
                body: [latestVersion],
            },
            { test: (u) => u.includes("/lanes?"), body: [] },
            { test: (u) => u.includes("/providers"), body: [] },
            {
                test: (u) => u.includes(`/workflows/${WORKFLOW_ID}/run`),
                // Return SSE wire format — runWorkflowStream reads this via resp.body.getReader()
                sseBody: {
                    type: "done",
                    request_id: "req_run_0001",
                    workflow_id: WORKFLOW_ID,
                    workflow_version: 3,
                    snapshot_version: 7,
                    plan_hash: "plan_abc123",
                    output: { message: "hello from the run panel", ok: true },
                },
            },
        ]);
        renderBuilder();
        await screen.findByText(WORKFLOW_ID, {}, { timeout: 2000 });

        await click(screen.getByRole("button", { name: /Run test/i }));
        await click(screen.getByRole("button", { name: /^Run$/i }));

        await screen.findByText("req_run_0001", {}, { timeout: 2000 });
        expect(screen.getByText("production-gateway · v3")).toBeTruthy();
        expect(screen.getByText(/v7/)).toBeTruthy();
    });

    test("unpublished workflow surfaces the REAL backend 409 (failed state)", async () => {
        stubFetch([
            {
                test: (u) => u.includes(`/workflows/${WORKFLOW_ID}/versions`),
                body: [latestVersion],
            },
            { test: (u) => u.includes("/lanes?"), body: [] },
            { test: (u) => u.includes("/providers"), body: [] },
            {
                test: (u) => u.includes(`/workflows/${WORKFLOW_ID}/run`),
                status: 409,
                body: {
                    error: "Workflow must be published before it can be run.",
                },
            },
        ]);
        renderBuilder();
        await screen.findByText(WORKFLOW_ID, {}, { timeout: 2000 });

        await click(screen.getByRole("button", { name: /Run test/i }));
        await click(screen.getByRole("button", { name: /^Run$/i }));

        await act(async () => {
            await Promise.resolve();
        });
        await waitFor(
            () => {
                expect(
                    screen.getByText(/Workflow must be published before it can be run\./),
                ).toBeTruthy();
            },
            { timeout: 2000 },
        );
    });
});
