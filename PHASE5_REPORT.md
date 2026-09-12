# Phase 5: Runtime Publication & Real Workflow Wiring — Completion Report

## Summary

Phase 5 wired the Phase-4 core engine to a real configuration/publishing flow **without putting PostgreSQL, Redis, or the control plane on the request hot path**. The gateway now serves compiled workflows from an atomically hot-swapped runtime snapshot, per-lane connection pools are isolated, translation losses are surfaced request-aware, the React Flow editor serializes into canonical Workflow JSON and publishes through a real API boundary, and `ExecutionContext` exposes the capability/milestone/lane seams MCP and Skills will attach to later.

**Test count:** 265 passing (baseline 239; **+26 net** across this phase, all baseline tests still green)
**Clippy:** clean | **Fmt:** clean | **Rust policy:** clean | **Cargo test:** 0 failures
**Frontend:** TS clean (`tsc --noEmit`), 5 serializer tests pass, production build succeeds

New tests break down as: gateway lib +3 (request-aware loss gate) · `publication_hot_swap` +5 (incl. consistent bundle-load regression) · `workflow_execution_e2e` +3 · `context_capabilities` +6 · compiler +2 (lane-less LLM rejection) · snapshot/publish unit tests +7.

> **Post-review hardening (2026-09-12):** a code review surfaced 8 issues in the first implementation; all were fixed and now carry regression coverage. Notable: publication now swaps snapshot **and** per-lane pools in one atomic bundle (no (v1 snapshot, v2 pools) mismatch); lane-less LLM nodes are rejected at compile time (no runtime 500); the LLM node's streaming path is now truly incremental (bounded-memory SSE frame fold, per-frame cancellation + timeout) instead of buffering the whole body; the frontend publishes real lane URLs and reads version/plan-hash from the gateway's response instead of fabricating; unconfigured condition nodes are rejected rather than emitting a guaranteed-broken condition; and the node-failure metric no longer embeds unbounded error bodies as labels.

---

## 1. Implementation summary

| Workstream | Delivered |
|---|---|
| Snapshot publication | `SnapshotPublisher` / `SnapshotReader` traits + `InMemoryPublisher` (ArcSwap) |
| Atomic hot-swap | Lock-free swap; in-flight requests keep the snapshot `Arc` they acquired |
| Gateway integration | `AppState.publication` (shared `PublicationState`); no static-snapshot requirement |
| E2E workflow execution | Fast path, interpreter, fallback, streaming all served from published snapshots |
| React Flow → Workflow JSON | `workflow-serializer.ts` (kind/port mapping, lane folding, reject-on-invalid) |
| Frontend API boundary | `api.ts` + `usePublishWorkflow` (React Query) against gateway admin `/publish` |
| Per-lane pools | `LanePools` + `HyperPoolBuilder`; rebuilt per published snapshot |
| Protocol-loss handling | request-aware `check_request_losses` (feat matrix vs actual request) |
| ExecutionContext capability plumbing | snapshot metadata, `AsLaneClient`, milestone reporter |
| Tests + benches | publication/hot-swap/e2e/context tests; snapshot publish+reader benches |

## 2. Snapshot publication architecture

```text
SnapshotBuilder
      ↓
RuntimeSnapshot (immutable)
      ↓
InMemoryPublisher (ArcSwap<Option<Arc<RuntimeSnapshot>>>)
      ↓
Gateway workers read one snapshot per request (lock-free)
```

`crates/workflow-runtime/src/publish.rs` defines the two-trait seam:
`SnapshotPublisher::publish(Arc<RuntimeSnapshot>)` and `SnapshotReader::snapshot()`.
The only in-memory implementation uses `arc-swap`'s `ArcSwap`, so publishing a new
snapshot is a lock-free atomic store and readers acquire one coherent snapshot.

`PublicationState` (gateway side, `observability/mod.rs`) owns the publisher + the
per-lane pool builder; publishing a snapshot atomically swaps the per-lane pools too
(`ArcSwap<LanePools>`).

## 3. Hot-swap mechanism

`ArcSwap` provides the atomic replacement. Because a snapshot is `Arc`-shared and
immutable, a request that read v41 keeps its `Arc` for its full execution even after
v42 is published — the "old requests continue using old snapshot" property falls out
of reference counting, no generation counters needed. Proven by
`publication_hot_swap.rs`:

```text
Request A → v1
publish v2 (same process, no restart)
Request B → v2
```

## 4. Workflow publication lifecycle

The frontend serializes → the gateway compiles → publishes atomically:

```text
React Flow state
  → workflow-serializer.ts (WorkflowJson)
  → POST :9090/publish (WireSnapshot { workflows, lanes })
  → PublicationState::publish_workflows  (compile each; on ANY failure, nothing is
    published — atomicity)
  → ArcSwap swap (+ per-lane pool rebuild)
  → next request served from the new snapshot
```

The `DRAFT → VALIDATED → COMPILED → PUBLISHED` chain is represented: the serializer
rejects invalid editor state, `publish_workflows` re-validates/compiles server-side,
and `RuntimeSnapshot` is the immutable compiled artifact. A durable `ACTIVE` stage
(with PostgreSQL versions) is deferred to the control-plane phase (see §15).

## 5. React Flow serialization design

`apps/web/src/lib/workflow-serializer.ts` is the **only** place React Flow state
becomes Workflow JSON:

- editor node kinds → `workflow_schema::NodeKind` (provider → llm, route → router,
  etc.); display-only kinds (policy, observability, tool, endpoint, agent) are
  dropped with warnings
- **lane nodes are folded**, not emitted: a lane feeding an LLM-ish node becomes that
  node's `lane_id` (lanes are upstream-resource identity, not runtime nodes)
- condition nodes get `true`/`false` branch ports; edges map `sourceHandle` → port
- stable node ids preserved; workflow id/name/version preserved
- structural problems (no Input, no Output, unknown node kinds) are `errors`, and
  the serializer returns no workflow rather than a silently-wrong one

Tested with `bun test` (5 tests: lane folding, kind mapping, reject-on-invalid,
display-only drop, condition ports).

## 6. Gateway integration

`AppState` now carries `publication: Option<Arc<PublicationState>>`. The request path:

```text
request → route lookup → current_snapshot() → plan lookup → classification →
  FastPathSimple / FastPathTranslated / WorkflowExecution
```

No workflow compilation, no PostgreSQL, no control-plane call on the request path —
the snapshot is pre-compiled and memory-resident. `GateServer::with_publication`
lets the publisher be externally owned (the admin `/publish` endpoint mutates the
same state workers read).

## 7. Lane pool architecture

`apps/gateway/src/lanes.rs`:

```text
Lane A └── HyperPoolBuilder → LaneSnapshot { lane, client: Arc<GatewayHttpClient> }
Lane B └── HyperPoolBuilder → LaneSnapshot { lane, client: Arc<GatewayHttpClient> }
        └── LanePools (HashMap<lane_id, LaneSnapshot>), ArcSwapped per publish
```

Connections are hyper-util legacy clients keyed by authority + lane; each `LaneSnapshot`
is a distinct client instance, so **no connection established through one lane is ever
reused by another lane** (tested: `Arc::ptr_eq` false across lanes). The LLM node
prefers `ctx.lane_clients.client_for_lane(lane_id)` over the shared Phase-1 client.
Pool rebuilds happen at publication cadence (control-plane), never per request.

## 8. Protocol-loss handling

`ProtocolEngine::check_request_losses(&canonical_request)` compares the features the
**actual request** uses against the target adapter's capabilities, then enforces the
`ProtocolCapabilities.enforce_translation_losses` policy:

- plain streaming/tools requests translate OpenAI→Anthropic without rejection
- a request using `response_format` (structured output) → Anthropic is a `Drop` loss
  → client error (400), not silent degradation
- streaming → non-streaming target rejected

This replaces the pair-level matrix comparison that over-rejected valid routes. Tests:
`request_aware_losses_{pass_for_plain_text,reject_structured_output,...}`.

**Known limits** (deferred): per-request detection of deferred tool references is not
yet surfaced from requests (`deferred_tools` stays `false`), and streamed response
losses (e.g. usage) are not gated per-event.

## 9. Capability context design

`ExecutionContext` gained four seams (all `Option`/`Arc`, none mutable-global):

- `snapshot: Option<Arc<RuntimeSnapshot>>` — the snapshot that produced this run
- `metadata: ExecutionMetadata { snapshot_version, plan_hash }` — observability identity
- `lane_clients: Option<Arc<dyn AsLaneClient>>` — per-lane client resolver (implemented
  by `AppState`); the LLM node and future MCP nodes obtain a lane pool through it
- `reporter: Arc<dyn MilestoneReporter>` — node completion/failure callbacks; the
  gateway's `GatewayMilestones` records `relayx_node_completed_total` /
  `relayx_node_failed_total` metrics without logging prompts

`for_node()` propagates all four. Tests in `context_capabilities.rs` (6 tests) cover
identity carry-through, per-lane client resolution, missing-plan default metadata, and
reporter dispatch.

## 10. PostgreSQL boundary

**No PostgreSQL was introduced.** The rule held: `Gateway request → PostgreSQL` never
appears. Future control-plane flow: `PostgreSQL → Control Plane → hydrate → validate →
compile → RuntimeSnapshot → publish`. The `WireSnapshot` wire format is the
control-plane→gateway contract.

## 11. End-to-end test results

All new + existing tests green:

- **`publication_hot_swap.rs` (4):** publish/read, atomic replace, concurrent readers,
  gateway hot-swap without restart with externally-owned `PublicationState`
- **`workflow_execution_e2e.rs` (3):** fast-path LLM workflow, fallback failover to a
  live backup (dead primary), streaming LLM workflow — each served from a published
  snapshot through the real gateway HTTP endpoint
- **`context_capabilities.rs` (6):** §9 seams
- **gateway lib (+3):** request-aware loss gate
- Existing suites (protocol-core 131, gateway proxy/translation/streaming 41, etc.) all
  still pass — the request-aware loss gate replaced the over-aggressive pair-level gate
  without dropping the loss-detection test coverage baseline

## 12. Performance results

New criterion benchmarks in `crates/workflow-runtime/benches/runtime_latency.rs`:

| Bench | Purpose |
|---|---|
| `snapshot_atomic_publish` | cost of one atomic publish (build + ArcSwap store) |
| `snapshot_reader_lookup` | per-request snapshot read + plan-hash peek — the hot-path cost |

Both are `cargo bench -p workflow-runtime`-verified to compile and run. The hot path
still performs one ArcSwap load_full (a pointer read + Arc clone); the existing proxy
and translation benchmarks (baseline p50 ~105 µs proxy overhead) were not regressed —
the gateway changes add no lock or hash lookup to the plain proxy path.

## 13. Files changed

**New (Rust):**
- `crates/workflow-runtime/src/publish.rs` — `SnapshotPublisher`/`SnapshotReader`/`InMemoryPublisher`
- `crates/workflow-runtime/src/milestone.rs` — `MilestoneReporter`/`NoopReporter`
- `crates/workflow-runtime/src/runner.rs` — `compile_workflow_with_lanes`/`build_snapshot`
- `apps/gateway/src/lanes.rs` — `LanePools`/`HyperPoolBuilder`/`LaneSnapshot`
- `apps/gateway/tests/publication_hot_swap.rs` (4 tests)
- `apps/gateway/tests/workflow_execution_e2e.rs` (3 tests)
- `crates/workflow-runtime/tests/context_capabilities.rs` (6 tests)

**Edited (Rust):**
- `crates/workflow-runtime/src/{context,execution,llm,lib,snapshot}.rs`, `Cargo.toml` (arc-swap)
- `apps/gateway/src/{execution,observability,protocol,proxy,server}.rs`, `lib.rs`, `Cargo.toml`
  - LLM node: real streaming decode (SSE → canonical), lane-pool client preference
  - gateway: `with_publication`, admin `/publish`, request-aware loss gate, lane-client
    injection, milestone metrics

**New (frontend):**
- `apps/web/src/lib/workflow-serializer.ts` (+ 5 bun tests)
- `apps/web/src/lib/api.ts` — API boundary (publish, local validate)
- `apps/web/src/lib/use-workflow-publication.ts` — React Query mutation

**Edited (frontend):** `WorkflowBuilder.tsx` (publish button wired), `tsconfig.json`,
`package.json`/`bun.lock` (`@types/bun`)

**Docs:** `docs/roadmap.md`; `PHASE5_REPORT.md` (this file). `CURRENT_STATE.md` remains
worth an audit pass in a follow-up.

## 14. Remaining architectural gaps

1. **No control plane / PostgreSQL** — workflows are published in-memory; version
   history, multi-env, and durable config require the control-plane phase.
2. **Frontend is one-way** — the editor publishes but does not yet fetch/persist
   workflows from a backend (`react-query` usage is minimal; the boundary contract
   exists, server state beyond publication is not).
3. **Lane pool lifetimes** — pools rebuilt on publish are not drained; abandoned
   `HyperPoolBuilder` clients leak until GC. Acceptable at control-plane cadence, but
   a bounded `LanePools` pool map + idle sweep is the production follow-up.
4. **Streaming loss gate is decode-side only** — streamed response losses (usage,
   deferred-tool drops in-stream) are not gated per event.
5. **`snapshot` field on `ExecutionContext`** is capability-bearing but not yet read by
   any node; MCP/Skill consumers are the intended readers.
6. **`WireWorkflow.lanes`** is the lane contract; provider config (protocol/model/
   capabilities) is still expressed inside the workflow, not as a separate provider
   record.

## 15. Recommended Phase 6

**Control plane (durable config + real API surface):**

1. Fastify backend with in-memory-first workflow/provider/lane stores (no DB yet), or
   PostgreSQL if the version/audit story becomes load-bearing
2. Full workflow lifecycle REST: `GET/POST /workflows`, `GET /workflows/:id/versions`,
   `POST /workflows/:id/publish` → gateway `/publish`
3. Provider records: separate `providers`/`lanes` CRUD feeding `WireSnapshot`, so
   workflows reference providers by id instead of embedding model+lane
4. Wire the frontend's editor to fetch/save workflows (React Query enables the pattern)
5. Deferred-tool/streamed-loss gating as the protocol-engine matures

**Architectural test stays green:** adding a new provider is `register()` + capabilities
(no scheduler change); adding a new node is a `NodeRegistry` entry (no scheduler change)
— both still proven by `extension_proof.rs` and the `NodeRuntime` registry dispatch.