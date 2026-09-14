/**
 * Schema-driven node Inspector (Phase 6.6 §8).
 *
 * Select a node → canonical node → NodeDefinition → typed form → canonical
 * config update. Editing "Model" writes `config.model`; editing the title
 * writes only presentation metadata. Validation runs locally per keystroke
 * (schema layer) and surfaces backend/compiler diagnostics verbatim.
 */

import { useEffect, useRef, useState } from "react";
import { AlertTriangle, Plus, Trash2, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { KV, SectionLabel } from "../primitives";
import { nodeMeta } from "./nodes";
import type { CanonicalConfig, CanonicalNode, InputVariable } from "@/lib/workflow/nodes";
import { getNodeDefinition, VARIABLE_TYPES, type FieldDef } from "@/lib/workflow/node-definitions";
import type { Issue } from "@/lib/workflow/validation";

function Group({ label, children }: { label: string; children: React.ReactNode }) {
    return (
        <div className="border-b border-border px-3 py-2.5 last:border-b-0">
            <SectionLabel>{label}</SectionLabel>
            <div className="mt-1.5">{children}</div>
        </div>
    );
}

const inputCls =
    "focus-ring h-7 w-full rounded-sm border border-border bg-canvas px-2 text-xs outline-none placeholder:text-muted-foreground focus:border-primary";

function Field({
    def,
    value,
    issue,
    onChange,
    laneOptions,
    providerOptions,
    modelOptions,
}: {
    def: FieldDef;
    value: unknown;
    issue?: Issue | undefined;
    onChange: (v: unknown) => void;
    laneOptions: { value: string; label: string }[];
    providerOptions: { value: string; label: string }[];
    modelOptions: { value: string; label: string }[];
}) {
    const refOptions =
        def.reference === "lanes"
            ? laneOptions
            : def.reference === "providers"
              ? providerOptions
              : def.reference === "models"
                ? modelOptions
                : (def.options ?? []);
    const isRefSelect =
        (def.reference === "lanes" ||
            def.reference === "providers" ||
            def.reference === "models") &&
        refOptions.length > 0;
    const optValue = typeof value === "string" ? value : String(value ?? "");
    const onOpt = (v: string) => onChange(v === "" ? undefined : v);
    return (
        <label className="block">
            <div className="flex items-center justify-between">
                <span className="label-xs">
                    {def.label}
                    {def.required ? <span className="text-fail"> *</span> : null}
                </span>
                {issue ? <span className="text-[10px] text-fail">{issue.message}</span> : null}
            </div>
            {def.type === "boolean" ? (
                <div className="mt-1 flex items-center gap-2">
                    <input
                        type="checkbox"
                        checked={Boolean(value)}
                        onChange={(e) => onChange(e.target.checked)}
                        className="size-3.5 accent-[var(--color-primary)]"
                    />
                    <span className="text-[11px] text-muted-foreground">enabled</span>
                </div>
            ) : def.type === "enum" || (isRefSelect && refOptions.length > 0) ? (
                <select
                    value={optValue}
                    onChange={(e) => onOpt(e.target.value)}
                    className={cn(inputCls, "appearance-none")}
                >
                    {!optValue ? <option value="">—</option> : null}
                    {refOptions.map((o) => (
                        <option key={o.value} value={o.value}>
                            {o.label}
                        </option>
                    ))}
                </select>
            ) : def.type === "number" || def.type === "integer" ? (
                <input
                    type="number"
                    value={
                        typeof value === "number" ? value : value === undefined ? "" : String(value)
                    }
                    min={def.min}
                    max={def.max}
                    step={def.step ?? (def.type === "integer" ? 1 : "any")}
                    onChange={(e) => {
                        const v = e.target.valueAsNumber;
                        onChange(Number.isFinite(v) ? v : undefined);
                    }}
                    className={cn(inputCls, "num")}
                />
            ) : (
                <input
                    value={typeof value === "string" ? value : ""}
                    placeholder={def.placeholder}
                    onChange={(e) => onChange(e.target.value)}
                    className={inputCls}
                />
            )}
            {def.help ? <p className="mt-1 text-[10px] text-muted-foreground">{def.help}</p> : null}
        </label>
    );
}

function InputVariables({
    nodeId,
    variables,
    issues,
    onConfigChange,
    config,
}: {
    nodeId: string;
    variables: InputVariable[];
    issues: Issue[];
    onConfigChange: (nodeId: string, config: CanonicalConfig) => void;
    config: CanonicalConfig;
}) {
    const update = (next: InputVariable[]) => {
        const c = structuredClone(config) as Extract<CanonicalConfig, { kind: "input" }>;
        c.variables = next;
        onConfigChange(nodeId, c);
    };
    const nameIssues = issues.filter((i) => i.field === "variables");
    return (
        <Group label="Input Variables">
            {variables.length === 0 ? (
                <p className="text-[11px] text-muted-foreground">
                    No expected input variables declared.
                </p>
            ) : (
                <div className="space-y-2">
                    {variables.map((v, i) => (
                        <div key={i} className="rounded-sm border border-border bg-canvas/60 p-2">
                            <div className="flex items-center gap-1.5">
                                <input
                                    value={v.name}
                                    placeholder="variable name"
                                    onChange={(e) => {
                                        const next = [...variables];
                                        next[i] = { ...v, name: e.target.value };
                                        update(next);
                                    }}
                                    className={cn(inputCls, "flex-1")}
                                />
                                <select
                                    value={v.type}
                                    onChange={(e) => {
                                        const next = [...variables];
                                        next[i] = { ...v, type: e.target.value as InputVariable["type"] };
                                        update(next);
                                    }}
                                    className={cn(inputCls, "w-[92px] appearance-none")}
                                >
                                    {VARIABLE_TYPES.map((o) => (
                                        <option key={o.value} value={o.value}>{o.label}</option>
                                    ))}
                                </select>
                                <button
                                    onClick={() => update(variables.filter((_, j) => j !== i))}
                                    className="focus-ring rounded-sm p-1 text-muted-foreground hover:text-fail"
                                    aria-label={`Remove variable ${v.name || i + 1}`}
                                >
                                    <Trash2 className="size-3.5" />
                                </button>
                            </div>
                            <div className="mt-1.5 flex items-center gap-1.5">
                                <input
                                    value={v.description ?? ""}
                                    placeholder="description (optional)"
                                    onChange={(e) => {
                                        const next = [...variables];
                                        next[i] = { ...v, description: e.target.value };
                                        update(next);
                                    }}
                                    className={cn(inputCls, "flex-1")}
                                />
                                <label className="flex items-center gap-1 text-[11px] text-muted-foreground">
                                    <input
                                        type="checkbox"
                                        checked={Boolean(v.required)}
                                        onChange={(e) => {
                                            const next = [...variables];
                                            next[i] = { ...v, required: e.target.checked };
                                            update(next);
                                        }}
                                        className="size-3 accent-[var(--color-primary)]"
                                    />
                                    req.
                                </label>
                            </div>
                            {nameIssues.map((iss, idx) => (
                                <p key={idx} className="mt-1 text-[10px] text-fail">{iss.message}</p>
                            ))}
                        </div>
                    ))}
                </div>
            )}
            <button
                onClick={() => update([...variables, { name: "", type: "string", required: false }])}
                className="focus-ring mt-2 flex items-center gap-1.5 rounded-sm border border-border px-2 py-1 text-[11px] text-muted-foreground hover:border-border-strong hover:text-foreground"
            >
                <Plus className="size-3" /> Add variable
            </button>
        </Group>
    );
}

export function Inspector({
    node,
    onConfigChange,
    onTitleChange,
    onClose,
    className,
    planHash,
    issues = [],
    laneOptions = [],
    providerOptions = [],
    modelOptions = [],
    focusOnMount = false,
    onFocusConsumed,
}: {
    node: CanonicalNode | null | undefined;
    onConfigChange: (nodeId: string, config: CanonicalConfig) => void;
    onTitleChange: (nodeId: string, title: string) => void;
    onClose?: () => void;
    className?: string;
    planHash?: string | null;
    issues?: Issue[];
    laneOptions?: { value: string; label: string }[];
    providerOptions?: { value: string; label: string }[];
    modelOptions?: { value: string; label: string }[];
    focusOnMount?: boolean;
    onFocusConsumed?: () => void;
}) {
    const titleInputRef = useRef<HTMLInputElement>(null);
    const [titleDraft, setTitleDraft] = useState(node?.presentation?.title ?? "");
    useEffect(() => {
        setTitleDraft(node?.presentation?.title ?? "");
    }, [node?.id, node?.presentation?.title]);

    // Auto-focus title input on double-click open. Runs once per trigger.
    useEffect(() => {
        if (focusOnMount && titleInputRef.current) {
            titleInputRef.current.focus();
            titleInputRef.current.select();
            onFocusConsumed?.();
        }
    }, [focusOnMount, node?.id, onFocusConsumed]);

    if (!node) {
        return (
            <aside className={cn("flex min-h-0 flex-col bg-panel", className)}>
                <div className="hairline-b flex h-9 items-center px-3">
                    <span className="label-xs">Inspector</span>
                </div>
                <div className="flex flex-1 items-center justify-center p-6 text-center">
                    <p className="max-w-[190px] text-[11px] leading-relaxed text-muted-foreground">
                        Select a node to edit its configuration. Runtime health/per-node metrics are
                        not shown — there is no per-node telemetry backend yet.
                    </p>
                </div>
            </aside>
        );
    }

    const def = getNodeDefinition(node.type);
    const meta = nodeMeta[node.type];
    const Icon = meta?.icon;
    const nodeIssues = issues.filter((i) => i.nodeId === node.id);

    const set = (name: string, value: unknown) => {
        const next = structuredClone(node.config) as CanonicalConfig;
        applyField(next, name, value);
        onConfigChange(node.id, next);
    };

    return (
        <aside className={cn("flex min-h-0 flex-col bg-panel", className)}>
            <div className="hairline-b flex h-9 items-center gap-2 px-3">
                {Icon ? <Icon className={cn("size-3.5", meta?.accent)} /> : null}
                <span className="label-xs flex-1 truncate">{def?.label ?? node.type}</span>
                {onClose ? (
                    <button
                        onClick={onClose}
                        className="focus-ring text-muted-foreground hover:text-foreground"
                    >
                        <X className="size-3.5" />
                    </button>
                ) : null}
            </div>

            <div className="min-h-0 flex-1 overflow-y-auto">
                <Group label="Title (display only)">
                    <input
                        ref={titleInputRef}
                        value={titleDraft}
                        onChange={(e) => {
                            setTitleDraft(e.target.value);
                            onTitleChange(node.id, e.target.value);
                        }}
                        placeholder="Never parsed into runtime config"
                        className={inputCls}
                    />
                </Group>

                <Group
                    label={`Configuration · ${def?.schemaVersion ? `v${def.schemaVersion}` : "v1"}`}
                >
                    <div className="space-y-2.5">
                        {def?.fields
                            .filter((f) => {
                                if (!f.dependsOn) return true;
                                const actual = valueOf(node.config, f.dependsOn.field);
                                // `dependsOn.value` is a `|`-separated list of acceptable values.
                                return f.dependsOn.value.split("|").includes(String(actual));
                            })
                            .map((f) => (
                                <Field
                                    key={f.name}
                                    def={f}
                                    value={valueOf(node.config, f.name)}
                                    onChange={(v) => set(f.name, v)}
                                    laneOptions={laneOptions}
                                    providerOptions={providerOptions}
                                    modelOptions={modelOptions}
                                    {...(nodeIssues.find((i) => i.field === f.name)
                                        ? { issue: nodeIssues.find((i) => i.field === f.name) }
                                        : {})}
                                />
                            ))}
                        {def?.fields.length === 0 ? (
                            <p className="text-[11px] text-muted-foreground">
                                This node has no configuration fields.
                            </p>
                        ) : null}
                    </div>
                </Group>

                {node.config.kind === "input" ? (
                    <InputVariables
                        nodeId={node.id}
                        variables={node.config.variables ?? []}
                        issues={nodeIssues}
                        onConfigChange={onConfigChange}
                        config={node.config}
                    />
                ) : null}

                {nodeIssues.length > 0 ? (
                    <Group label="Issues">
                        <div className="space-y-1.5">
                            {nodeIssues.map((i, idx) => (
                                <div
                                    key={idx}
                                    className={cn(
                                        "flex gap-2 rounded-sm border px-2 py-1.5 text-[11px]",
                                        i.severity === "error"
                                            ? "border-fail/40 bg-fail/8 text-fail"
                                            : "border-warn/40 bg-warn/8 text-warn",
                                    )}
                                >
                                    <AlertTriangle className="mt-px size-3 shrink-0" />
                                    <span>{i.message}</span>
                                </div>
                            ))}
                        </div>
                    </Group>
                ) : null}

                {def?.note ? (
                    <Group label="Runtime">
                        <p className="text-[11px] leading-relaxed text-muted-foreground">
                            {def.note}
                        </p>
                    </Group>
                ) : null}

                <Group label="Execution">
                    <KV
                        k="Plan hash"
                        v={planHash ? planHash.slice(0, 12) : "not compiled"}
                        mono={false}
                        tone={planHash ? "ok" : "neutral"}
                    />
                    <KV k="Served by" v="Rust gateway data plane" />
                </Group>
            </div>
        </aside>
    );
}

function valueOf(config: CanonicalConfig, name: string): unknown {
    switch (config.kind) {
        case "input":
            if (name === "inputType") return config.inputType;
            if (name === "description") return config.description;
            return undefined;
        case "llm":
            return config.config[name as keyof typeof config.config];
        case "router":
            return name === "strategy" ? config.strategy : undefined;
        case "transform":
            return name === "operation" ? config.operation : undefined;
        case "condition":
            return config.condition[name as keyof typeof config.condition];
        case "mcp":
            return config.tool[name as keyof typeof config.tool];
        case "skill":
            return config.skill[name as keyof typeof config.skill];
        case "fallback":
            return config.fallback[name as keyof typeof config.fallback];
        case "retry": {
            if (name.startsWith("target."))
                return config.target[name.slice(7) as keyof typeof config.target];
            return config.policy[name as keyof typeof config.policy];
        }
        default:
            return undefined;
    }
}

function applyField(config: CanonicalConfig, name: string, value: unknown): void {
    switch (config.kind) {
        case "input":
            if (name === "inputType") config.inputType = value as string;
            if (name === "description") config.description = value as string;
            break;
        case "llm":
            (config.config as unknown as Record<string, unknown>)[name] = value;
            break;
        case "router":
            if (name === "strategy") config.strategy = value as never;
            break;
        case "transform":
            if (name === "operation") config.operation = value as never;
            break;
        case "condition":
            (config.condition as unknown as Record<string, unknown>)[name] = value;
            break;
        case "mcp":
            (config.tool as unknown as Record<string, unknown>)[name] = value;
            break;
        case "skill":
            (config.skill as unknown as Record<string, unknown>)[name] = value;
            break;
        case "fallback":
            if (name === "rounds")
                config.fallback.rounds = Number.isFinite(Number(value)) ? Number(value) : 1;
            break;
        case "retry":
            if (name.startsWith("target."))
                (config.target as unknown as Record<string, unknown>)[name.slice(7)] = value;
            else (config.policy as unknown as Record<string, unknown>)[name] = value;
            break;
    }
}
