/**
 * Workflow CRUD, validation, publish, rollback, and run routes.
 */

import type { FastifyInstance } from "fastify";
import type { Pool } from "pg";
import * as repo from "../../db/repositories";
import { createPublishService } from "../../domain/publish";
import type { GatewayClient } from "../../gateway/client";
import { createWorkflowSchema, publishSchema } from "../schemas";
import { versionForJson } from "../utils/json-helpers";

export type WorkflowDeps = {
  pool: Pool;
  gateway: GatewayClient;
  publish: ReturnType<typeof createPublishService>;
};

export function registerWorkflowRoutes(app: FastifyInstance, deps: WorkflowDeps): void {
  const { pool, gateway, publish } = deps;

  // ── Workflows CRUD ──────────────────────────────────────────────────
  app.get("/workflows", async () =>
    (await pool.query("SELECT * FROM workflows ORDER BY created_at DESC")).rows,
  );

  app.post("/workflows", async (req, reply) => {
    const parsed = createWorkflowSchema.safeParse(req.body);
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    // The frontend may create its durable row UNDER the editor's own id
    // (e.g. "production-gateway") so later GET/POST /workflows/:id match.
    // Without this, every publish creates an orphan UUID row (review #3).
    const id = parsed.data.id !== undefined ? parsed.data.id : undefined;
    const wf = await repo.workflows.create(pool, parsed.data.project_id, parsed.data.name, id);
    return reply.code(201).send(wf);
  });

  app.get("/workflows/:id", async (req, reply) => {
    const wf = await repo.workflows.get(pool, (req.params as { id: string }).id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    return wf;
  });

  app.put("/workflows/:id", async (req, reply) => {
    const { id } = req.params as { id: string };
    const body = req.body as { name?: string };
    if (!body.name) return reply.code(400).send({ error: "name required" });
    const wf = await repo.workflows.update(pool, id, body.name);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    return wf;
  });

  app.delete("/workflows/:id", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    // Never delete an ACTIVE (Production) workflow — the gateway is serving
    // its published snapshot. Deleting would leave a live-but-orphaned plan
    // (review: delete guard). The authoritative active-pointer row is
    // workflow_active (workflows.status is a denormalized auxiliary flag that
    // publish doesn't reliably set). Deactivate/roll back first.
    if (await repo.workflows.getActiveVersion(pool, id)) {
      return reply.code(409).send({
        error: "Cannot delete an active workflow. Roll back to deactivate it first.",
      });
    }
    // Cascade deletes (ON DELETE CASCADE) remove versions, publications,
    // the active pointer, and any run records. The gateway's in-memory
    // snapshot may keep serving the plan until the next publish/rehydrate —
    // the deleted workflow is no longer in workflow_active, so the next
    // coherent bundle drops it (documented simple-delete behavior).
    await repo.workflows.remove(pool, id);
    return reply.code(204).send();
  });

  app.get("/workflows/:id/versions", async (req, reply) => {
    const { id } = req.params as { id: string };
    if (!(await repo.workflows.get(pool, id)))
      return reply.code(404).send({ error: "workflow not found" });
    return repo.workflows.listVersions(pool, id);
  });

  app.post("/workflows/:id/versions", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const body = (req.body ?? {}) as { workflow_json?: Record<string, unknown> };
    // Atomic version allocation avoids the concurrent-publish race (D2).
    const v = await repo.workflows.createNextVersion(pool, id, body.workflow_json ?? {});
    if (wf.status !== "active") await repo.workflows.setStatus(pool, id, "draft");
    return reply.code(201).send(v);
  });

  /** Validate + compile against the gateway without publishing. */
  const validateCompile = async (
    req: import("fastify").FastifyRequest,
    reply: import("fastify").FastifyReply,
  ) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const body = (req.body ?? {}) as {
      workflow_json?: Record<string, unknown>;
      version?: number;
    };
    const workflowJson = body.workflow_json ?? {};
    // Resolve the version whose stored JSON MATCHES the submitted content, so
    // the recorded plan_hash always belongs to the version it is stored under
    // (review #5). No match → refuse rather than hash mismatched content.
    const version = body.version ?? (await versionForJson(pool, id, workflowJson));
    if (version === null) {
      return reply.code(400).send({
        error: "no stored version matches the submitted workflow_json (create a version first)",
      });
    }
    const wire = await publish.buildWire({ workflowId: id, workflowJson, version });
    if ("error" in wire) return reply.code(400).send({ error: wire.error });
    const result = await gateway.validate(wire);
    if (!result.ok) return reply.code(400).send({ error: result.error });
    const planHash =
      result.workflows.find((w) => w.workflow_id === id)?.plan_hash ?? null;
    await repo.workflows.updateVersionStatus(pool, id, version, "compiled", planHash);
    // A validate/compile must NOT degrade an active parent (review #9).
    if (wf.status !== "active") await repo.workflows.setStatus(pool, id, "compiled");
    return {
      status: "compiled",
      snapshot_version: result.snapshot_version,
      plan_hash: planHash,
      workflows: result.workflows,
    };
  };

  app.post("/workflows/:id/validate", validateCompile);
  app.post("/workflows/:id/compile", validateCompile);

  app.post("/workflows/:id/publish", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const parsed = publishSchema.safeParse(req.body ?? {});
    if (!parsed.success) return reply.code(400).send({ error: parsed.error.message });
    const version =
      parsed.data.version ?? (await versionForJson(pool, id, parsed.data.workflow_json));
    if (version === null) {
      return reply.code(400).send({
        error: "no stored version matches the submitted workflow_json (create a version first)",
      });
    }
    const result = await publish.publish({
      workflowId: id,
      version,
      workflowJson: parsed.data.workflow_json,
    });
    if (result.status === "error") return reply.code(400).send({ error: result.error });
    return {
      status: "published",
      workflow_id: id,
      workflow_version: version,
      snapshot_version: result.snapshot_version,
      plan_hash: result.plan_hash,
      published_at: result.published_at,
    };
  });

  app.post("/workflows/:id/rollback", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });
    const active = await repo.workflows.getActiveVersion(pool, id);
    if (!active) return reply.code(409).send({ error: "no active version to roll back from" });
    // Roll back only to a version that was genuinely validated/compiled
    // (status), never to a raw draft that may carry a stale plan_hash from
    // the COALESCE in updateVersionStatus (review D1).
    const previous = (await repo.workflows.listVersions(pool, id))
      .filter(
        (v) =>
          v.version < active.workflow_version &&
          (v.status === "compiled" || v.status === "active" || v.status === "published") &&
          Boolean(v.plan_hash),
      )
      .sort((a, b) => b.version - a.version)[0];
    if (!previous)
      return reply.code(409).send({ error: "no prior validated version to republish" });

    // Rollback = publish a previous valid version (spec §18), never a mutation.
    const result = await publish.publish({
      workflowId: id,
      version: previous.version,
      workflowJson: previous.workflow_json,
    });
    if (result.status === "error") return reply.code(500).send({ error: result.error });
    return {
      status: "rolled_back",
      to_version: previous.version,
      snapshot_version: result.snapshot_version,
      plan_hash: result.plan_hash,
    };
  });

  app.post("/workflows/:id/run", async (req, reply) => {
    const { id } = req.params as { id: string };
    const wf = await repo.workflows.get(pool, id);
    if (!wf) return reply.code(404).send({ error: "workflow not found" });

    // Run only ever executes the ACTIVE (published) version. A draft/dirty
    // canvas is never silently published or exec'd (§11). The ACTIVE check is
    // the authoritative backend gate, not a frontend invention.
    const active = await repo.workflows.getActiveVersion(pool, id);
    if (!active) {
      return reply
        .code(409)
        .send({ error: "Workflow must be published before it can be run." });
    }

    const body = ((req.body ?? {}) as { body?: unknown }).body ?? null;

    // ?stream=true pipes the gateway's SSE token stream through to the
    // browser without buffering.
    const { stream } = (req.query ?? {}) as { stream?: string };
    if (stream === "true") {
      let gatewayResp: Response;
      try {
        gatewayResp = await gateway.runStream({ workflow_id: id, body });
      } catch (e) {
        // A stream that fails BEFORE hijack never reaches the pump, so the
        // durable run record must be created + finalized here — otherwise
        // this failure leaves NO trace, contradicting the non-stream path's
        // "a failed run still appears in run history" contract.
        const message = e instanceof Error ? e.message : String(e);
        const failedRow = await repo.runs
          .create(pool, {
            workflow_id: id,
            workflow_version: active.workflow_version,
            snapshot_version: active.snapshot_version,
            plan_hash: active.plan_hash,
            input_body: body,
          })
          .catch(() => null);
        if (failedRow) {
          await repo.runs
            .update(pool, failedRow.id, {
              status: "failed",
              error: message,
              completed_at: new Date().toISOString(),
            })
            .catch((err) =>
              console.error("[runs] failed to finalize stream-failure record:", err),
            );
        }
        return reply.code(502).send({ error: message });
      }
      const upstream = gatewayResp.body;
      if (!upstream) {
        return reply
          .code(502)
          .send({ error: "gateway returned an empty stream" });
      }

      reply.hijack();
      // @fastify/cors writes headers on `reply`, which hijacking bypasses;
      // carry the same reflect-origin policy onto the raw response.
      if (req.headers.origin) {
        reply.raw.setHeader("Access-Control-Allow-Origin", req.headers.origin);
        reply.raw.setHeader("Vary", "Origin");
      }

      // Durable run record (Phase 6.8): created once the gateway accepted the
      // stream; finalized by the pump with the terminal status. Best-effort —
      // run history must never break the response path. Created AFTER hijack
      // so a DB failure can't orphan the gateway stream with no HTTP response
      // to point at (review: orphaned stream). `runRow` may be null on DB
      // failure; the pump then skips the finalizing UPDATE.
      const runRow = await repo.runs
        .create(pool, {
          workflow_id: id,
          workflow_version: active.workflow_version,
          snapshot_version: active.snapshot_version,
          plan_hash: active.plan_hash,
          input_body: body,
        })
        .catch(() => null);
      reply.raw.writeHead(200, {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        Connection: "keep-alive",
      });
      const reader = upstream.getReader();
      // Terminal-status detection on the raw SSE passthrough: the gateway
      // emits `event: done` / `event: error` as the terminal frames, so a
      // byte scan of the forwarded chunks is enough to record the honest run
      // status without parsing the stream (which would add hot-path cost).
      // `remainder` carries a trailing partial line across chunk boundaries
      // so a terminal marker split by the transport is still detected.
      let sawDone = false;
      let sawError = false;
      let remainder = "";
      const decoder = new TextDecoder();
      const scanLines = (text: string) => {
        for (const line of text.split("\n")) {
          if (line === "event: done") sawDone = true;
          if (line === "event: error") sawError = true;
        }
      };
      const pump = async () => {
        try {
          while (true) {
            if (reply.raw.destroyed) break;
            const { done, value } = await reader.read();
            if (done) {
              // Scan trailing remainder for a split terminal marker.
              scanLines(remainder);
              break;
            }
            const chunk = decoder.decode(value);
            const text = remainder + chunk;
            scanLines(text);
            remainder = text.includes("\n")
              ? text.slice(text.lastIndexOf("\n") + 1)
              : text;
            // Respect backpressure: Node's `write()` returns `false` when the
            // socket's high-water mark is hit (await `drain` before reading
            // more), `true` when it flushed directly. A slow/stalled client
            // must not let the run's full output accumulate in memory. A
            // destroyed socket must not leave the pump awaiting a drain that
            // never fires (otherwise the run row stays 'running' forever).
            if (!reply.raw.write(value)) {
              await new Promise<void>((resolve) => {
                let settled = false;
                const finish = () => {
                  if (settled) return;
                  settled = true;
                  reply.raw.off("drain", finish);
                  reply.raw.off("close", finish);
                  resolve();
                };
                reply.raw.once("drain", finish);
                reply.raw.once("close", finish);
              });
            }
          }
        } catch (err) {
          // Mid-stream failure must still reach the browser as a terminal
          // SSE `error` event — a silent stream end makes the frontend mark a
          // completed run as failed. A user abort (browser closed the socket /
          // reader cancelled) is NOT an error: don't emit a spurious error
          // event, and let the finally block classify via `reply.raw.destroyed`.
          const isAbort =
            err instanceof Error &&
            (err.name === "AbortError" || /^abort(ed)?\b/i.test(err.message ?? ""));
          if (!isAbort) sawError = true;
          if (!reply.raw.destroyed && !isAbort) {
            try {
              const wire = `event: error\ndata: ${JSON.stringify({
                error: err instanceof Error ? err.message : String(err),
              })}\n\n`;
              reply.raw.write(wire);
            } catch {
              /* response already gone */
            }
          }
        } finally {
          // Finalize the durable run record with the honest terminal status:
          // a terminal `done` frame is a completed run; a terminal `error`
          // frame (or mid-stream failure) is failed; a socket destroyed
          // before either terminal event is a user cancel; a clean close
          // with NO terminal event is a truncated stream = failure, never a
          // silent success (the gateway must always emit a terminal frame).
          const completedAt = new Date().toISOString();
          try {
            if (!runRow) {
              /* no row to finalize (creation failed) */
            } else if (sawError) {
              await repo.runs.update(pool, runRow.id, { status: "failed", error: "stream error", completed_at: completedAt });
            } else if (sawDone) {
              await repo.runs.update(pool, runRow.id, { status: "completed", completed_at: completedAt });
            } else if (reply.raw.destroyed) {
              await repo.runs.update(pool, runRow.id, { status: "cancelled", completed_at: completedAt });
            } else {
              // Stream ended cleanly without a terminal event (gateway close).
              await repo.runs.update(pool, runRow.id, { status: "failed", error: "stream ended without terminal event", completed_at: completedAt });
            }
          } catch {
            /* run history is best-effort; the stream already succeeded */
          }
          if (!reply.raw.destroyed) reply.raw.end();
        }
      };
      // On pump failure OR browser abort the raw stream closes (destroy); make
      // sure the upstream reader is cancelled so the gateway's run task sees
      // `tx.closed()` and aborts the in-flight provider request instead of
      // streaming it to completion (provider spend after cancel).
      reply.raw.on("close", () => {
        if (!reply.raw.writableEnded) reader.cancel().catch(() => {});
      });
      pump().catch(() => reader.cancel());
      return reply;
    }

    // Durable run record for the non-streaming path (best-effort).
    // Created BEFORE the gateway call so a failed run still appears in run
    // history (review: failed runs must leave a record). `create` is awaited
    // (one round-trip, yields the row ID). The finalizing `update` is awaited
    // too — the expensive gateway call is already past, one more round-trip
    // before the response makes run history deterministic (a fire-and-forget
    // update could race the client's follow-up GET /runs and show a stale
    // 'running' row). A failure in either path is non-fatal.
    const runRow = await repo.runs
      .create(pool, {
        workflow_id: id,
        workflow_version: active.workflow_version,
        snapshot_version: active.snapshot_version,
        plan_hash: active.plan_hash,
        input_body: body,
      })
      .catch(() => null);

    let result: Awaited<ReturnType<GatewayClient["run"]>>;
    try {
      result = await gateway.run({ workflow_id: id, body });
    } catch (e) {
      // An unexpected throw (transport, timeout, malformed response) must
      // not leave a forever-'running' row. Finalize as failed — mirrors the
      // typed `!result.ok` path below.
      const message = e instanceof Error ? e.message : String(e);
      if (runRow) {
        await repo.runs
          .update(pool, runRow.id, {
            status: "failed",
            error: message,
            completed_at: new Date().toISOString(),
          })
          .catch((err) =>
            console.error("[runs] failed to finalize run record after throw:", err),
          );
      }
      return reply.code(502).send({ error: message });
    }

    // Finalize the run record with the honest outcome — failed for gateway
    // errors, completed for success. Failures are logged, never swallowed, so
    // a stuck 'running' row is at least visible in the logs.
    const completedAt = new Date().toISOString();
    if (!result.ok) {
      if (runRow) {
        await repo.runs
          .update(pool, runRow.id, {
            status: "failed",
            error: result.error,
            completed_at: completedAt,
          })
          .catch((e) =>
            console.error("[runs] failed to finalize run record as failed:", e),
          );
      }
      return reply.code(400).send({ error: result.error });
    }

    if (runRow) {
      await repo.runs
        .update(pool, runRow.id, {
          status: "completed",
          output: result.output,
          completed_at: completedAt,
        })
        .catch((e) =>
          console.error("[runs] failed to finalize run record as completed:", e),
        );
    }

    return {
      status: "ok",
      request_id: result.request_id,
      workflow_id: result.workflow_id,
      workflow_version: active.workflow_version,
      snapshot_version: result.snapshot_version,
      plan_hash: result.plan_hash,
      output: result.output,
    };
  });
}
