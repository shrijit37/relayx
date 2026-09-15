# Phase: Core Engine Foundation — Completion Report
> **Archived snapshot** — historical record, not current truth. Current state: [../state.md](../state.md).

## Summary

The core engine now has stable extension contracts, a versioned Execution IR with deterministic hashing, a compiler with lane and schema validation, fast-path classification, immutable runtime snapshots, fallback/retry nodes, and end-to-end gateway workflow execution. All gates pass (239 tests, clippy, fmt, rust policy).

**An earlier version of this report claimed "Gateway can execute a compiled plan" before that path was actually wired.** A code review (run at max effort, findings verified against source) caught that the wiring was dead: the gateway snapshot was hard-coded `None`, the node registry was never consulted by the executor, fast-path metadata fabricated a protocol, and retry invented a `"default"` lane. All of those have since been fixed and covered by tests.

**Test count:** 239 passing (baseline 208; +31 net across this phase incl. fixes)
**Clippy:** clean | **Fmt:** clean | **Rust policy:** clean

Test breakdown by crate: protocol-core 131 · workflow-schema 12 · workflow-runtime 42 · relay-gateway 50 · mock-upstream 4. All 208 baseline tests still green.

---

## What was implemented

### 1. Stable extension contracts

**Node trait** (`nodes/trait_node.rs`): `NodeExecutor` trait with `kind()`, `capabilities()`, `execute()` methods. `NodeRegistry` holds `Box<dyn NodeExecutor>` and resolves a custom kind string. `NodeKind::Custom` nodes are dispatched **through the registry** by `execute_node` (before the built-in match arms); built-in kinds keep the match. Foreign nodes never touch the scheduler internals — verified by a test that runs a `Custom` node through `NodeRuntime`.

**Capability model** (`capability.rs`): `Capabilities` struct covering streaming, tools, structured_output, reasoning, vision, audio, citations, deferred_tools, cache_hints. `excess()` and `is_satisfied_by()` for compiler validation. Converts from `protocol_core::ProtocolCapabilities`.

**Provider model** (`provider.rs`): `ProviderEntry` with id, protocol, base_url, model, capabilities, lane_id. `ProviderRegistry` keyed by id. Adding a provider is one struct + one `register()` call — zero changes to core.

### 2. Execution IR hardening

`ExecutionPlan` now carries:
- `plan_version: u64` (format version, currently `PLAN_VERSION = 1`)
- `plan_hash: String` (SHA-256 of deterministic node/edge serialization — same input produces same hash, verified by test)
- `classification: PlanClassification` — `FastPathSimple`, `FastPathTranslated`, or `WorkflowExecution`
- `fast_path: Option<FastPathMetadata>` — for fast paths, carries the **real** `LlmConfig` of the single LLM node (protocol, model, lane, stream flag) so execution behaves identically to the interpreter
- a `NodeRegistry` used to dispatch `Custom` nodes

Accessor methods: `plan_version()`, `plan_hash()`, `classification()`, `fast_path()`, `nodes()`, `edges()`, `registry()`.

### 3. Compiler (`compiler.rs`)

`compile_workflow(workflow, ctx) → Result<ExecutionPlan, CompileError>`:

1. Schema validation (delegates to `Workflow::validate()` — cycles, reachability, ports, duplicates)
2. Lane reference validation (Llm node `lane_id` must resolve in the provided `LaneRegistry`)
3. Execution plan compilation via `ExecutionPlan::compile()`

`CompileContext` holds an `Arc<LaneRegistry>`. `CompileError` has `Schema`, `LaneNotFound`, `Internal`, `Workflow` variants, all convertible to `WorkflowError`.

### 4. Fast path (`fast_path.rs`)

`execute_fast_path(plan, ctx, input)`:

- Checks `plan.classification()` — refuses `WorkflowExecution` plans immediately
- Reads pre-resolved `FastPathMetadata` from the plan
- Calls the LLM node directly — no topological loop, no port data store, no edge evaluation
- Structurally obvious: one function, one call, one error path

### 5. Runtime snapshot (`snapshot.rs`)

`RuntimeSnapshot` bundles:
- Wall version (u64, monotonic)
- Pre-compiled plans (`HashMap<String, Arc<ExecutionPlan>>`)
- Lane registry (`Arc<LaneRegistry>`)
- Provider entries (`HashMap<String, Arc<ProviderEntry>>`)
- Publication timestamp

`RuntimeSnapshotBuilder` constructs snapshots with a builder pattern. Published atomically via `Arc` swap. Gateway reads one snapshot per request — no database on the hot path.

### 6. Fallback + Retry nodes

**Fallback** (`nodes/fallback.rs`): Cycles the provider list `rounds` times; tries each provider in order and produces the first success. Each provider carries its own optional protocol override. A missing lane or client is a hard skip and skips to the next provider.

**Retry** (`nodes/retry.rs`): Re-invokes the configured `target` LLM lane up to `max_attempts` times with a configurable delay. `on_provider_error` gates whether provider failures trigger further attempts; internal/client errors surface immediately (so a broken config fails fast, not silently).

Both wired into the `execute_node()` match in `execution.rs` and declared in `workflow_schema::NodeKind` and `NodeConfig`.

### 7. Gateway workflow execution

`GatewayServer::with_snapshot()` builds a server that carries a compiled `RuntimeSnapshot`. A route configured with `workflow_id` (no `lane` needed) dispatches to `execute_workflow`, which injects the gateway's HTTP client into the execution context and runs the plan (fast path or interpreter). The `workflow_route_e2e` integration test proves the path end-to-end: route → snapshot → plan → JSON response.

### 8. Fast-path benchmark (`benches/runtime_latency.rs`)

`plan_compile_with_classification` bench: compiles a 3-node LLM workflow and measures compile + classification time. Benchmarks classification determinism and compile overhead.

---

## What already existed (unchanged, protected)

- `ConfigSnapshot` — immutable TOML-to-snapshot at startup
- `ProtocolEngine` — OpenAI Chat / Anthropic Messages / OpenAI Responses translation
- `NodeRuntime::execute()` — topological interpreter with port-based data flow
- 6 built-in node implementations (LLM, Transform, Condition, Router, MCP, Skill)
- All 208 original tests continue passing

---

## File changes summary

**New files:**
- `crates/workflow-runtime/src/nodes/trait_node.rs`
- `crates/workflow-runtime/src/capability.rs`
- `crates/workflow-runtime/src/provider.rs`
- `crates/workflow-runtime/src/compiler.rs`
- `crates/workflow-runtime/src/fast_path.rs`
- `crates/workflow-runtime/src/snapshot.rs`
- `crates/workflow-runtime/src/nodes/fallback.rs`
- `crates/workflow-runtime/src/nodes/retry.rs`
- `crates/workflow-runtime/tests/extension_proof.rs`
- `apps/gateway/src/execution.rs`
- `apps/gateway/tests/workflow_route_e2e.rs`

**Edited files:**
- `crates/workflow-runtime/src/execution.rs` — IR hardening, plan version/hash/classification, classify_plan, compute_plan_hash, custom dispatch, registry field
- `crates/workflow-runtime/src/lib.rs` — added modules, expanded public API
- `crates/workflow-runtime/src/nodes/mod.rs` — added fallback, retry modules
- `crates/workflow-runtime/Cargo.toml` — added sha2
- `crates/workflow-schema/src/lib.rs` — added Fallback/Retry/Custom NodeKinds + configs; open node model
- `crates/workflow-runtime/benches/runtime_latency.rs` — classification bench
- `apps/gateway/src/config/mod.rs` — `workflow_id` on routes, optional lane for workflow routes
- `apps/gateway/src/proxy/mod.rs` — workflow-route dispatch + real HTTP client injection
- `apps/gateway/src/server/mod.rs` — `with_snapshot()`, `AppState.client` as `Arc`

---

## Definition-of-done checklist

| Item | Status |
|---|---|
| Stable Node contract exists | ✅ `NodeExecutor` trait + `NodeRegistry` |
| Stable Provider contract exists | ✅ `ProviderEntry` + `ProviderRegistry` |
| Stable Protocol Adapter contract exists | ✅ Already existed (ProtocolEngine) |
| Capability model is defined | ✅ `Capabilities` struct |
| Execution IR is implemented and versioned | ✅ `PLAN_VERSION`, `plan_hash`, `classification` |
| Workflow schema can compile into IR | ✅ `ExecutionPlan::compile()` |
| Compiler performs semantic validation | ✅ `compile_workflow()` — schema + lane validation (incl. fallback/retry lanes) |
| Compilation is deterministic | ✅ Same workflow → same hash (tested) |
| Compiled plans can execute | ✅ `NodeRuntime::execute()` + `execute_fast_path()` |
| Immutable runtime snapshots exist | ✅ `RuntimeSnapshot` + `RuntimeSnapshotBuilder` |
| Gateway can execute a compiled plan | ✅ `GatewayServer::with_snapshot()` + `workflow_id` route; e2e test proves the path (route → snapshot → plan → JSON response) |
| Simple workflows use dedicated fast path | ✅ `execute_fast_path()` — uses the real LLM config (no fabricated protocol/stream) |
| Provider execution streams correctly | ✅ Existing ProtocolEngine streaming preserved |
| Fallback/retry work through execution model | ✅ Fallback + Retry node kinds + implementations (+ policy tests) |
| Existing protocol functionality intact | ✅ Zero changes to protocol-core |
| Existing tests continue passing | ✅ 208 baseline all green |
| New tests exist | ✅ 239 total (+31: registry-through-engine, retry policy, fast-path config preservation, gateway workflow e2e) |
| Performance benchmarks exist | ✅ Classification + compile bench |
| Adding a test provider doesn't change core executor | ✅ `extension_proof.rs` — provider registered outside core |
| Adding a test node doesn't change scheduler internals | ✅ `extension_proof.rs` — `UppercaseNode` through `NodeRegistry` consulted by the engine |
| No database on the hot path | ✅ `RuntimeSnapshot` is Arc-shared, in-memory |

---

## Remaining gaps / recommended next phase

1. **Control-plane snapshot publication** — `GatewayServer::with_snapshot` requires the caller to build and hand over a snapshot; a true control plane that compiles from a DB and hot-swaps via `Arc` (or `ArcSwap`) is the next step
2. **Compiler pipeline: React Flow → Workflow JSON** — TypeScript compiler that transforms editor state into `workflow_schema::Workflow`
3. **Dynamic config reload** — Hot-swap `RuntimeSnapshot` via `tokio::sync::watch` or `ArcSwap`
4. **MCP/Skill integration** — Wire real `McpToolExecutor` and `SkillLoader` into `ExecutionContext`
5. **Full control plane** — PostgreSQL, provider/lane CRUD, workflow versioning
6. **Frontend ↔ backend wiring** — API layer connecting the React Flow editor to the control plane

---

## Key architectural decisions

- **The NodeRegistry is the extension boundary and the engine actually dispatches through it** — `Custom` node kinds resolve via the registry; built-ins use the match. Verified by a test that runs a foreign node through `NodeRuntime`.
- **Node kinds are open** — `NodeKind::Custom` + `CustomConfig` let foreign nodes enter a workflow without widening the match.
- **SHA-256 hash of node/edge serialization** is deterministic and cheap. No external hash dependencies beyond `sha2`.
- **Fast path carries the real LLM config** — a fast-path plan behaves identically to the interpreter (same protocol, streaming flag, lane, model). No fabricated defaults.
- **Retry targets are explicit** — `RetryConfig.target` names the LLM lane to re-invoke; `on_provider_error`/`on_timeout` gate retry eligibility.
- **Snapshot builder is explicit** — no `ArcSwap` yet; hot-swap is a follow-up.
- **Lane validation lives in the compiler** — runs at compile time, covers LLM, fallback providers, and retry targets.
