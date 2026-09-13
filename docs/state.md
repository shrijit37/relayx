# STATE.md — Project State

## Status

**Phase 1 COMPLETE. Phase 2 COMPLETE. Phase 3 (lanes/routing) PARTIAL (lanes exist, no health/WireGuard). Phase 4 (workflow compiler) COMPLETE. Phase 5 (runtime publication & frontend wiring) COMPLETE. Phase 6 (control plane & durable configuration) COMPLETE. Phase 6.5 (reality audit) COMPLETE — see [phase-6.5-reality-audit.md](../docs/phase-6.5-reality-audit.md) for the evidence-based reality baseline. Phase 6.5 implementation COMPLETE — see [PHASE6.5_IMPLEMENTATION_REPORT.md](../PHASE6.5_IMPLEMENTATION_REPORT.md).**

> **Reality-check summary (Phase 6.5 complete):** The frontend is no longer a mock. The workflow editor loads persisted workflows from the backend, Save/Validate/Publish are real control-plane operations, and the Run path executes the published ACTIVE version through the real gateway (control-plane `/run` → gateway admin `/run` → workflow runtime → provider, with the real envelope shown in the UI). All fabricated data (`relay-data.ts`, inline fixtures, `Math.sin` time series) is gone: every page either fetches real backend rows or displays an honest "not available yet" state. Run history, telemetry, MCP/Skills/policies/secrets remain UNAVAILABLE (no backend) and are presented as such — never fabricated.

### Phase 6 — Control plane & durable configuration (COMPLETE)

See [`PHASE6_REPORT.md`](../PHASE6_REPORT.md) for the full completion report. Highlights:

- **Durable control plane** — `apps/control-plane/` (TypeScript/Fastify) persists workflows, versions, providers, lanes, policies, and publications to PostgreSQL; the gateway data plane stays entirely memory-resident.
- **Atomic publish pipeline** — the control plane builds a `WireSnapshot` (lanes as `WireLane` with resolved authorization), `POST`s to gateway `/validate` (compile-only, deterministic plan hash recorded), then `/publish` (atomic snapshot + lane-pool swap). A failed publish leaves the previous runtime active.
- **Credential references** — lanes store `credential_ref` (env/vault), resolved to an `Authorization` header at publish time; raw secrets never appear in workflow JSON, API responses, logs, or metrics.
- **Workflow lifecycle** — `draft → validated → compiled → published → active`; versions immutable; rollback republishes a previous validated version.
- **Frontend wired to real backend** — versions page + workflows index fetch from the control plane; publish returns backend-authoritative version/plan-hash. **Phase 6.5 closes the mock gap:** the editor loads persisted versions into the canvas, Save/Validate are wired end-to-end, and a real Run contract (control-plane → gateway → provider) executes the published ACTIVE version with the real envelope shown in the UI. Management pages show real persisted rows or honest unavailable states; `relay-data.ts` is deleted.
- **Gateway restart preservation** — the control plane rehydrates the last ACTIVE version of every workflow on boot.

**Test count (Phase 6): Rust 270 passing, zero failures.** Baseline 267 → +3 (control-plane e2e, gateway `/run`). **Control plane: 12 integration tests** against a real Postgres 16 + in-process mock gateway. **Frontend: tsc clean, 19 serializer+run-state tests, production build clean.**

### Phase 5 — Runtime publication & frontend wiring (COMPLETE)

See [`PHASE5_REPORT.md`](../PHASE5_REPORT.md) for the full completion report. Highlights:

- **Snapshot publication + atomic hot-swap** — `SnapshotPublisher`/`SnapshotReader` + `InMemoryPublisher` (ArcSwap). Gateway serves compiled workflows from an atomically swapped `RuntimeSnapshot`; no PostgreSQL/Redis/control-plane on the request hot path.
- **Per-lane connection pools** — `LanePools` + `HyperPoolBuilder`; a distinct Hyper client per lane (no cross-lane connection reuse), rebuilt on each published snapshot, swapped atomically with the snapshot as one bundle.
- **End-to-end compiled workflow execution** — fast path, interpreter, fallback, and streaming all served from published snapshots through the real gateway HTTP endpoint.
- **Protocol translation-loss gate (request-aware)** — `ProtocolEngine::check_request_losses` rejects only the features a request actually uses that the target can't represent.
- **React Flow → Workflow JSON** — `workflow-serializer.ts` (kind/port mapping, lane folding, condition validation, reject-on-invalid editor state).
- **Frontend API boundary** — `api.ts` + `usePublishWorkflow` (React Query) against gateway admin `/publish`; version/plan-hash read from server response (not fabricated).
- **`ExecutionContext` capability plumbing** — snapshot identity, execution metadata (snapshot_version/plan_hash), lane-aware client resolver (`AsLaneClient`), milestone reporter.

**Test count (Phase 5): 265 passing, zero failures.** Baseline 239 → +26.

### Phase 1 — High-performance HTTP proxy data plane
- TOML config → immutable snapshot, no DB dependency
- Route matching + lane-based upstream forwarding
- HTTP/1.1 streaming proxy (zero buffering, streaming passthrough)
- Per-request timeout (`tokio::time::timeout` wrapping handler)
- Per-frame streaming timeout (`FrameTimeoutStream` — custom `Stream` impl)
- Upstream connection pooling via `hyper_util::client::legacy::Client`
- Structured tracing (JSON) + Prometheus metrics (13 metric families)
- Typed error hierarchy with JSON error responses
- Mock LLM upstream (JSON + SSE modes, configurable latency/errors, `Drop` for clean shutdown)
- 38 integration/unit tests, 4 load tests
- Criterion benchmarks: **gateway overhead ~105µs (non-streaming), ~21µs (SSE streaming)**
- CI: SHA-pinned actions, least-privilege permissions, concurrency control, 30min timeout

### Phase 2 — Protocol engine (COMPLETE)

**`crates/protocol-core/`** — canonical protocol model + 3 adapters + SSE parser + benchmarks.

**`apps/gateway/src/protocol.rs`** — live gateway protocol translation engine. Wires `protocol-core`
into the proxy hot path: routes can declare `source_protocol`/`target_protocol`, translating
requests/response/stereams through the canonical model. Streaming is event-by-event (SSE parser
drives incremental translation, no full-stream buffering). Phase 1 passthrough remains intact for
routes without protocol config.

| Milestone | Status | What was built |
|---|---|---|
| M2.1 Canonical Model | ✅ | `CanonicalRequest`, `CanonicalResponse`, `ContentBlock` (7 variants: Text, Image, Audio, Reasoning, ToolUse, ToolResult, ToolReference), `Message`, `ToolDefinition`, `ToolUseBlock`, `ToolResultBlock`, `ToolReference`, `AudioContent`, `AudioSource`, `ReasoningContent`, `Usage`, `FinishReason`, `CanonicalStreamEvent` (14 variants incl. AudioDelta, ReasoningDelta, ReasoningSignature), `ProtocolCapabilities`, `ProviderExtensions`, `ProtocolEngineError`, `LossyTranslation`, `LossPolicy` |
| M2.2 OpenAI Chat Completions | ✅ | Request decoder, response encoder, streaming encoder, tool call encoding, usage encoding, `decode_request_messages`, `decode_tool_choice` helpers |
| M2.3 Anthropic Messages | ✅ | Request encoder, response decoder, streaming encoder, tool use/result translation, cache usage fields, `Thinking`/`ThinkingDelta`/`SignatureDelta` support, `encode_system_instruction`, `encode_request_messages`, `encode_tools_and_choice` helpers |
| M2.4 SSE Parser + Stream Engine | ✅ | `StreamingSseParser` (protocol-framing-level, not transport-chunk-level), `format_sse_event`, `format_done_event` |
| M2.5 Conformance Tests | ✅ | 36 translation e2e tests, 26 streaming boundary tests, 13 canonical/error tests, golden fixtures, round-trip tests, capability loss detection, enforcement tests, error handling, edge cases |
| M2.6 OpenAI Responses | ✅ | Full adapter: `ResponsesRequest`, `ResponsesResponse`, `ResponsesItem`, `ResponsesContentPart`, `ResponsesStreamEvent`, `decode_request`, `decode_response`, `encode_response`, `encode_stream_event` — handles `input_text`/`output_text`/`input_audio`/`function_call`/reasoning items, `tool_choice` decoding |
| M2.7 Performance Benchmarks | ✅ | Criterion benchmarks for request decoding, response encoding, stream event encoding, cross-adapter translation |
| M2.8 Code Quality | ✅ | Split oversized adapter functions (`encode_request` → 4 helpers, `decode_request` → `decode_request_messages` + `decode_tool_choice`), `enforce_translation_losses()` wired into adapter boundaries |
| M2.10 Gateway Integration | ✅ | `ProtocolEngine` in gateway: route-level `source_protocol`/`target_protocol` config, request decode→canonical→target encode, response decode→canonical→client encode, SSE event-by-event stream translation, error mapping (`ProtocolEngineError` → `GatewayError` → HTTP) |

**Total test count (Phase 2 snapshot): 197 tests passing, zero failures.** 131 in protocol-core, 39 in gateway, 12 in workflow-schema, 4 in mock-upstream. Includes 5 property tests (SSE parser non-panic, format→parse roundtrip, canonical serde roundtrip). **Workspace-wide count today: 265 (see Phase 5 section).**

Phase 2 compliance status (post-audit):
- ✅ Gateway now depends on and invokes `protocol-core` (was pure passthrough)
- ✅ Real non-streaming translation OpenAI Chat ↔ Anthropic works through the live gateway
- ✅ Streaming translation path implemented event-by-event (no full-stream buffering)
- ✅ `translation_losses()` expanded to detect streaming/tools/multimodal/reasoning/usage/deferred losses
- ✅ Reasoning capability correctly declared for Anthropic (thinking blocks fully supported)
- ✅ OpenAI tool-call streaming index preserved (was hardcoded `0`)
- ✅ Responses `tool_choice` decoded (was dropped)
- ✅ Property tests added (SSE parser + canonical roundtrip)
- ⚠️ Loss policy enforcement is engine-level (`check_losses()`); per-feature message-level
  enforcement is documented as a known limitation (see `docs/protocols.md`)

This document is the source of truth for current implementation state. Update it after meaningful work.

### Workflow schema (`crates/workflow-schema/`)

Typed workflow definition crate (12 tests passing). Provides:

- `Workflow`, `Node`, `Edge` with typed `NodeKind` (11 variants: Input, Output, LLM, Router, Transform, Condition, MCP, Skill, Fallback, Retry, Custom) and `NodeConfig`
- Port-based data flow model (`PortType`: Message, Stream, ToolCall, ToolResult, Json, Bool)
- `Workflow::validate()` — graph validation: duplicate IDs, unknown refs, cycles, reachability, dead-end detection

### Workflow runtime (`crates/workflow-runtime/`)

Execution engine (52 tests: unit + integration + extension-proof + context capabilities). Provides:

- `ExecutionPlan::compile(Workflow)` — topological sort via Kahn's algorithm, versioned (`PLAN_VERSION`), deterministic content hash, execution-path classification (fast path / workflow)
- `compile_workflow(workflow, ctx)` — schema + lane validation (including compile-time rejection of lane-less LLM nodes when 0 or multiple lanes exist)
- `NodeRuntime::execute()` — walks nodes in topological order with cancellation + deadline support, port-based data flow, conditional edges
- Real node implementations: **LLM** (real provider calls + incremental SSE streaming), Transform, Condition, Router, Fallback, Retry; Custom nodes dispatch through the `NodeRegistry` (extension boundary)
- `ExecutionContext` — lane registry, lane-aware client resolver (`AsLaneClient`), snapshot identity, execution metadata (snapshot_version/plan_hash), milestone reporter
- `RuntimeSnapshot` / `RuntimeSnapshotBuilder` — immutable bundle of plans + lanes + providers; published atomically via `SnapshotPublisher`/`InMemoryPublisher` (ArcSwap)
- `ProviderEntry`/`ProviderRegistry` — stable provider extension boundary (adding a provider is register + capabilities, no scheduler change)
- MCP/Skill nodes return explicit "not yet executed" behavior until the MCP/Skills runtime ships

### Frontend (`apps/web/`)

React 19 + TanStack Start + TanStack Router + React Flow + Vite (scaffolded via Lovable.dev).

**Pages (15 user-facing routes) — backend-driven or honest unavailable (Phase 6.5):**
- **Fully real:** `/workflows` (list from control plane) ✅
- **Real:** `/workflows/$workflowId` (load latest version → canvas, Save/Validate/Publish/Run all real) ✅
- **Real:** `/workflows/$workflowId/versions` (version list + plan hash from control plane) ✅
- **Real (persisted rows):** `/providers`, `/lanes`, `/health` (via control-plane `/system/health` probe) ✅
- **Backend-driven overview:** `/` (workflows/lanes/providers counts + real health; KPIs honestly unavailable) ✅
- **Honest unavailable (no backend yet):** `/runs`, `/runs/$runId`, `/observability`, `/mcp`, `/skills`, `/policies`, `/secrets` ✅ (no fabricated data)

**Workflow editor:**
- Full React Flow canvas with 16 node kind variants, drag-and-drop, edge connections, Inspector panel
- **Workflow serialization + deserialization** — `workflow-serializer.ts` maps React Flow ↔ canonical Workflow JSON (lane folding, condition validation, reject-on-invalid, unknown-kind skip-with-warning)
- **API boundary** — `api.ts` (publish, validate, save, run, fetch, system-health) + React Query hooks wiring the editor to the control plane
- **Load** — `deserializeWorkflow(latest.workflow_json)` reconstructs the canvas from the backend's latest version
- **Save** — creates an immutable version (`POST /workflows/:id/versions`); in `new` mode it creates the workflow row first, then navigates to the durable id
- **Validate** — real `POST /workflows/:id/validate` → real plan hash / real rejection
- **Run** — real control-plane `POST /workflows/:id/run` → gateway admin `/run` → workflow runtime → provider; the UI shows the real execution envelope (request id, snapshot version, plan hash, output) and surfaces real errors (409 unpublished, 404 unknown, provider 5xx). Abort cancels the real request.

**No fabricated data:** `relay-data.ts` is deleted; `graph.ts` holds only a 2-node empty starter; inline page fixtures (secrets/health/lanes/policies/settings) are gone; `Math.sin` time series are gone.

**Tests:** serializer + run-state reducer tests (`bun test`, `apps/web`), control-plane run integration tests, gateway `/run` integration test. TS clean, production build clean.

## Current decisions

| Area | Decision | Status |
|---|---|---|
| Frontend canvas | React Flow / xyflow | Decided |
| Data plane | Rust | Decided |
| Async runtime | Tokio | Decided |
| HTTP foundation | Hyper/Tower; Axum where useful | Decided |
| Proxy option | Evaluate Pingora for dedicated proxy path | Evaluate |
| Control plane | TypeScript + Fastify | Preferred |
| Persistent DB | PostgreSQL | Preferred |
| Cache/coordination | Redis only where justified | Preferred |
| Workflow runtime | Compiled IR/execution plan | Decided |
| Routing abstraction | Lane | Decided |
| MCP | Dynamic discovery + deferred loading semantics | Decided |
| Skills | Progressive discovery/loading | Decided |
| Protocol strategy | Canonical core + provider-specific extensions | Decided |
| Streaming | First-class | Decided |

For detailed rationale on each decision, see the [ADRs](./adr-0001-stack.md) and the [Decision Log](#decision-log) below.

## Decision log

Compact log of decisions. Detailed rationale belongs in ADR files.

| Decision | State |
|---|---|
| React Flow for visual graph | Accepted |
| Rust data plane | Accepted |
| TypeScript control plane | Preferred |
| Lane = provider/endpoint + network + policy + pool | Accepted |
| Compiled workflow IR | Accepted |
| Progressive MCP discovery | Accepted |
| Progressive Skill loading | Accepted |
| Canonical protocol + extensions | Accepted |
| Separate control/data planes | Accepted |
| No synchronous DB in hot path | Accepted |
| Runtime plugin installation | Deferred / restricted |

## Implementation phases

See [Roadmap](./roadmap.md) for the full implementation phase breakdown (Phase 0-8) with task-level checkboxes.

## Open questions

1. Whether Pingora should be the primary data-plane HTTP layer or only a specialized proxy component.
2. Which canonical event schema best preserves Anthropic/OpenAI/other provider semantics.
3. How much workflow execution belongs in Rust vs a separate runtime.
4. Whether MCP tool execution should occur inside the gateway or in sandboxed workers.
5. Which vector/index strategy provides the best tool/skill retrieval latency and recall.
6. How provider-specific reasoning/caching/tool-reference semantics should be represented without loss.
7. Whether Redis is needed initially or can be delayed.

## Invariants to protect

- no synchronous DB in normal proxy path
- no full response buffering for streaming
- no silent capability loss in protocol translation
- no unbounded dynamic tool exposure
- no untrusted arbitrary plugin execution in-process
- workflow version must be pinned per request

## Claude Code Automation

Current Claude Code configuration for this repository. Update this table whenever `.claude/` config changes (per the Documentation synchronization rule in AGENTS.md).

| Component | Status | Path |
|-----------|--------|------|
| settings.json (hooks + permissions) | Installed | `.claude/settings.json` |
| rust-policy hook | Installed | `.claude/hooks/check-rust-policy.sh` |
| architecture-guard skill | Installed | `.claude/skills/architecture-guard/SKILL.md` |
| scaffold-phase skill | Installed | `.claude/skills/scaffold-phase/SKILL.md` |
| protocol-fidelity-reviewer subagent | Installed | `.claude/agents/protocol-fidelity-reviewer.md` |
| hot-path-auditor subagent | Installed | `.claude/agents/hot-path-auditor.md` |

Hooks active:

- PostToolUse: architectural-rule reminder on every Edit/Write
- PostToolUse: `.claude/hooks/check-rust-policy.sh` on every Edit/Write (fails on `dead_code` suppression, `todo!`, `unimplemented!`, `.unwrap()`, `.expect()`)
- PreToolUse: block edits to `Cargo.lock` / `pnpm-lock.yaml` / `pnpm-lock.yml` / `bun.lockb` / `yarn.lock`
- PreToolUse: block edits to `.env` files
- Git pre-commit: same rust-policy checker via `.githooks/pre-commit` (`git config core.hooksPath .githooks`)

MCP servers (context7, GitHub) will be added at Phase 0 start — they require `claude mcp add`, which modifies the global Claude Code config rather than this repository.
