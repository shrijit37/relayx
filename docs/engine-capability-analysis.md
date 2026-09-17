# Engine Capability Analysis — Core Engine Offerings & Proven Custom-Node Capability

> **Status:** living · **Verified:** 2026-09-16 · **Purpose:** Source-of-truth map of what the workflow-runtime core engine actually offers today (with file:line evidence), and what it has proven capable of via an out-of-process custom-node implementation — with zero core-engine changes.

---

## 1. Executive Summary {#summary}

The relay-x core engine is a **node-graph execution runtime** with a compiled, immutable IR. It executes real LLM providers over HTTP, translates between three wire protocols in both directions, supports streaming (SSE) as a first-class path, and runs a deliberately isolated **Custom-node extension system** where extension code executes **out-of-process** via Unix-domain-socket RPC — never compiled into the gateway.

### What the core engine inherently offers (all executable, all tested)

| Capability | Where | Verified by |
|------------|-------|-------------|
| Graph execution (topo order, port routing, conditional edges) | `workflow-runtime/src/execution.rs` | `integration.rs` suite |
| Real LLM calls — OpenAI Chat, Anthropic Messages, OpenAI Responses | `protocol-core/src/adapters/*`, `nodes/llm.rs` | mitm/mock upstream tests |
| Bidirectional protocol translation (canonical model) | `protocol-core/src/canonical.rs`, `protocol-core/src/adapters/*` (`openai_chat`, `anthropic_messages`, `openai_responses`) | adapter conformance tests |
| Streaming SSE translation, event-by-event (bounded memory) | `workflow-runtime/src/fast_path.rs`, `protocol-core/src/sse.rs` | streaming integration tests |
| Fallback, Retry, Router (RoundRobin/FirstMatch), Condition, Transform nodes | `workflow-runtime/src/nodes/*.rs` | unit tests per node |
| Immutable snapshot + atomic hot-swap publication | `workflow-runtime/src/publish.rs`, `snapshot.rs` | `publication_state*` tests |
| Extension system (Custom nodes, out-of-process RPC) | `workflow-runtime/src/extension/{mod,worker}.rs`, `context.rs` | `integration.rs` custom-node tests (see §6) |

### What the core engine proves with a Custom-node implementation (zero core changes)

A full, working **Custom node** pipeline exists end-to-end — publish → compile → snapshot → gateway → UDS worker RPC — and is proven by tests. The only thing holding Custom nodes back from "fully featured" territory is that **no real extension executor/worker exists to call**; the transport, registry, validator, and hard-fail-closed semantics are all real and tested.

---

## 2. Node Types — What Each Does {#node-types}

The node catalog lives in `crates/workflow-schema/src/lib.rs`. Node **kinds** are the 11-variant `NodeKind` enum (`lib.rs:32`); each is paired with a strongly-typed config via the tagged `NodeConfig` enum (`lib.rs:68`) and its `validate()` (`lib.rs:428`).

| `NodeKind` (lib.rs) | `NodeConfig` variant | Executor file | Real behavior |
|---------------------|----------------------|---------------|---------------|
| `Input` | `Input(InputConfig)` | `execution.rs` (built-in) | Passes workflow input through unchanged |
| `Output` | `Output(OutputConfig)` | `execution.rs` (built-in) | Emits the node's message as the workflow result |
| `Llm` | `Llm(LlmConfig)` | `nodes/llm.rs` | Real HTTP call to a provider lane; streaming + buffered; protocol + lane + model from config |
| `Condition` | `Condition(ConditionConfig)` | `nodes/condition.rs` | Field comparison → true/false port; 8 operators (`ConditionOp`) |
| `Transform` | `Transform(TransformConfig)` | `nodes/transform.rs` | Passthrough / Extract / Merge / Filter on JSON |
| `Router` | `Router(RouterConfig)` | `nodes/router.rs` | FirstMatch or RoundRobin across `output_ports` |
| `Fallback` | `Fallback(FallbackConfig)` | `nodes/fallback.rs` | Ordered provider lane tries, multiple rounds |
| `Retry` | `Retry(RetryConfig)` | `nodes/retry.rs` | Re-invokes target LLM with policy (max attempts, delay, on-timeout/on-provider-error) |
| `Mcp` | `Mcp(McpConfig)` | `nodes/mcp.rs` | Delegates to `McpToolExecutor` trait — **trait defined, no implementor in repo** |
| `Skill` | `Skill(SkillConfig)` | `nodes/skill.rs` | Delegates to `SkillLoader` trait — **trait defined, no implementor in repo** |
| `Custom` | `Custom(CustomConfig)` | `execution.rs:428` → `extension/mod.rs` | Out-of-process extension registry lookup, validator-before-executor (§4) |

### 2.1 LLM Node — the hot path
`LlmConfig` (`workflow-schema/src/lib.rs` LLMConfig) carries: `protocol` (default per lane), `model`, `lane_id`, `temperature`, `max_tokens`, `stream`. Execution (`nodes/llm.rs:14`):

1. Resolve protocol (default OpenAI Chat if unset)
2. Resolve lane (explicit `lane_id`, else the single registered lane — errors if 0 or >1 lanes without explicit id)
3. Build **canonical** request from input (text/image/audio/tool blocks via `ContentBlock`)
4. Encode to target wire protocol through `protocol-core` adapters
5. HTTP POST with timeout + cancellation + deadline
6. Decode response: buffered (full body, gzip-aware) or streaming (SSE frame-by-frame, incremental, bounded frame timeout `StreamingSseParser`)
7. Emit client-native SSE `token` events through the wire channel when streaming
8. Map OpenAI `Responses`/`Anthropic` stream events → canonical `CanonicalStreamEvent` (text deltas, tool-call deltas, reasoning/signature, usage)

### 2.2 Router / Condition — graph control
- Router (`nodes/router.rs`): `FirstMatch` always selects port 0; `RoundRobin` cycles `route_0..route_N` via an `AtomicUsize` (`router.rs`). Output port count is derived from edge topology at compile (`execution.rs`). No weighted/content routing.
- Condition (`nodes/condition.rs`): `RuntimeValue` → operator → bool → port `true`/`false`. Condition nodes' `true`/`false` edges compile to auto-generated `PortEquals` conditions (`execution.rs`), so inactive branches are skipped at runtime — proven by `test_condition_true_branch` and `test_router_round_robin_three_ports`.

### 2.3 Fallback / Retry — resilience
- Fallback (`nodes/fallback.rs`): each lane tried in order per round; lanes share the LLM node path (`super::llm::execute`). Skips missing lanes; fails closed with typed errors. `FallbackConfig { providers, rounds }`.
- Retry (`nodes/retry.rs`): `max_attempts` loops against a target `LlmConfig`; `should_retry()` honors `on_timeout`/`on_provider_error`. Never retries client/internal errors.

---

## 3. Execution Pipeline {#pipeline}

The engine is `input → validate → compile → snapshot → execute`.

### 3.1 Compile (`workflow-runtime/src/execution.rs`)
- `ExecutionPlan::compile(&Workflow)` — schema validation, then builds `ExecNode`/`ExecEdge` IR.
- **Topological sort** (Kahn) refuses cycles; deterministic SHA-256 **plan hash** (`compute_plan_hash`).
- **Classification** (`classify_plan`/`PlanClassification`): single-LLM, no-condition, single-lane graphs become `FastPathSimple`/`FastPathTranslated`; everything else is `WorkflowExecution`.
- `compile_workflow_with_lanes` (in `compile.rs`) wires lane access + extension registry + `ExecutionPlan::compile`.

### 3.2 Execute
`NodeRuntime::execute(ctx, input)` (in `execution.rs` / `NodeRuntime`) runs nodes in topo order, feeding port data through a `Store`. Conditional edges skip downstream nodes (`skip` set). Cancellation (`cancel_token`), deadline enforcement, milestone reporting (`MilestoneReporter`), and per-node timing are all live.

### 3.3 Fast path vs interpreter
- `PlanClassification::FastPathSimple/FastPathTranslated` → **fast_path.rs** — bypasses the interpreter: direct `super::llm::execute`, real config, real streaming (`stream` preserved).
- `PlanClassification::WorkflowExecution` → full `NodeRuntime` interpreter.
- Fast path disqualifiers: any Condition/Router node, any MCP/Skill/Custom node, or anything beyond `Input → LLM → Output`. (Custom nodes always take the full interpreter + extension path.)

### 3.4 Snapshots & publication (the seam)
- `RuntimeSnapshot` (immutable, `Arc`); `RuntimeSnapshotBuilder` compiles a coherent set of plans.
- `InMemoryPublisher` → `ArcSwap` atomic swap (`publish.rs`); gateway holds `PublicationState` with `ArcSwap<PublishedBundle>` (snapshot + lane pools swapped as one unit).
- Control plane builds the wire bundle → `validate` (compile-only) → `publish` (atomic swap). Failure is fail-closed: a bad plan never publishes, active runtime stays untouched.

---

## 4. Custom Node Extension System — What It Actually Proves {#custom-system}

This is the centerpiece of "what the engine proves capable of with a custom node implementation, no core changes."

### 4.1 The extension contract (`workflow-runtime/src/extension/mod.rs`)
- `ExtensionValidator` — optional pre-execution validator trait (`mod.rs:37`). Runs BEFORE the executor; a validation failure rejects the node, executor never runs.
- `ExtensionExecutor` (`mod.rs:51`) — actual execution (out-of-process).
- `ExtensionSpec { kind, version, validator, executor }` (`mod.rs:77`) — registration spec per kind.
- `ExtensionRegistry` (`mod.rs:112`) — `register`, `get`, `kinds`, `specs_snapshots`.

### 4.2 Wire transport (`workflow-runtime/src/extension/worker.rs`)
- `UnixSocketExecutor` (`worker.rs:85`) — connects to a worker via Unix socket, one round-trip per call.
- Length-prefixed JSON frames (`[u32 BE len][json]`), `DEFAULT_RPC_TIMEOUT` 30s, `MAX_RESPONSE_FRAME` 64 MiB.
- Errors surface as typed `NodeError::Extension` — connection-refused, timeout, worker crash ("closed mid-response") all tested.

### 4.3 Execution path (proven end-to-end)
```
Custom node config (ext_kind + payload)
  → compile-time: kind resolution against ExtensionRegistry (non-blocking warning if missing)
  → runtime execute_node (execution.rs custom arm)
      → registry.get(kind)                 (fail-closed if no registry / kind missing)
      → validator.validate() (optional)    (runs first)
      → executor.execute()                 (never runs if validator fails)
  → NodeOutput
```

Proof of capability via tests (see §6 for the full evidence table):
- Custom node with a registered executor executes and returns output.
- Missing registry → typed fail-closed error (never fabricated success).
- Validator-fail blocks the executor; validator-before-executor ordering is asserted.
- Plan hash is sensitive to custom config payload (deterministic, changes with payload).
- `UnixSocketExecutor` real round-trip over an actual UDS server is tested (`worker.rs` tests).

### 4.4 What is NOT implemented (honest gaps)
- **No extension executor/worker binary ships in the repo** — the `ExtensionExecutor`/worker must be provided by the deployment. No `McpToolExecutor`, no `SkillLoader`, no concrete `ExtensionExecutor` for a real tool exists in source.
- Frontend: **no `custom` editor kind** — the React Flow editor cannot create/edit custom nodes (`node-definitions.ts` has no `custom` entry; the serializer is loss-preventing on them). Custom nodes must come from JSON/API.
- No per-extension process lifecycle (pooling/spawn) — connect-per-call, 30s default RPC timeout (documented in `worker.rs`).

---

## 5. Protocol Engine — Translation Coverage {#protocol}

`protocol-core` provides the canonical model + adapters (`protocol-core/src/adapters/mod.rs`):

| Adapter dir | Wire protocol | Direction tested |
|-------------|---------------|------------------|
| `openai_chat` | OpenAI Chat Completions | request encode/decode, response encode/decode, SSE chat stream |
| `openai_responses` | OpenAI Responses API | request/response + Responses SSE |
| `anthropic_messages` | Anthropic Messages | request/response + Anthropic SSE |

`ProtocolEngine::from_pair(source, target)` builds a translation engine; `check_request_losses()` rejects lossy translations (e.g. structured-output to a target that can't represent it); decode/encode go through canonical. Streaming is translated **event-by-event** with bounded memory — a core tenet.

---

## 6. Test Evidence — Capability Is Proven, Not Assumed (edited by Shrijit) {#evidence} 

All claims below cite the actual test suites in `crates/workflow-runtime/tests/` (and `src/**/tests/`). These run via `cargo test --all-features --workspace`.

### 6.1 Custom-node / extension capability tests (the "proven" set)

| Test | File | What it proves |
|------|------|----------------|
| `custom_node_with_registry_executes` | `workflow-runtime/tests/integration.rs` | Registered custom executor runs and returns correct `RuntimeValue` |
| `custom_node_without_registry_returns_error` | `integration.rs` | No registry → typed fail-closed error |
| `custom_node_unregistered_kind_returns_error` | `integration.rs` | Kind not registered → typed fail-closed error |
| `custom_node_failing_executor_propagates_error` | `integration.rs` | Executor `Err` → `NodeError::Extension` propagates |
| `validator_called_before_executor` | `integration.rs` | Validator runs first |
| `validator_fail_blocks_executor` | `integration.rs` | Validator failure blocks executor (never runs) |
| `custom_config_ext_kind_serde_roundtrip` | `integration.rs` | `ext_kind` serializes properly on the wire |
| `extension_registry_roundtrip_via_snapshot` | `integration.rs` | Extension specs survive snapshot build (kind+version) |
| `custom_node_plan_hash_includes_payload` | `integration.rs` | Plan hash is deterministic AND payload-sensitive |
| `unix_socket_executor_round_trip` | `extension/worker.rs` tests | Real UDS worker round-trip works |
| `unix_socket_executor_connection_refused` | `extension/worker.rs` tests | Fail-closed on missing socket |

### 6.2 Core engine tests (the "offers" set — sampling)
- Graph validation: duplicate IDs, cycles, unreachable/dead-end nodes (`workflow-schema` tests).
- Router round-robin cycling across 3 ports, 6 runs (`test_router_round_robin_three_ports`).
- Condition true/false branch selection incl. integer precision (`test_integer_condition_equality`).
- LLM node with mock upstream: buffered + streaming, protocol config preserved on fast path (`fast_path` tests, `llm.rs` tests).
- Fast path vs interpreter classification (`PlanClassification` tests).
- Snapshot atomicity: `publish_replaces_snapshot`, `publish_then_read` (`snapshot.rs`/`publication` tests).
- Concurrent executions run 5 workflows on one runtime (`test_concurrent_executions`).
- Cancellation: cancelled token returns `WorkflowError::Cancelled` (`test_cancellation`).

---

## 7. Bottom Line {#bottom-line}

1. **The core engine is execution-complete for the interpreter + fast path** and ships a tested protocol translation engine across 3 wire formats with streaming.
2. **Custom nodes are fully wired end-to-end and proven** (publish → snapshot → gateway → UDS RPC → executor), fail-closed, validator-first, with deterministic plan hashing that reacts to config. This is the core's proof that a custom implementation works **with no core-engine changes**.
3. **The only missing pieces for a shipping custom-node product are external** (a real extension worker/executor + a frontend `custom` editor kind) — the engine-side contract, transport, and semantics are done and tested.
