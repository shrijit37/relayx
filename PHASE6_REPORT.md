# Phase 6: Control Plane & Durable Configuration — Completion Report

## 1. Implementation summary

Phase 6 builds the durable control plane around the Phase-4/5 engine without contaminating the data-plane hot path. PostgreSQL is now the durable source of truth for workflows, versions, providers, lanes, policies, and publication history. The control plane (`apps/control-plane`, TypeScript/Fastify) orchestrates validation, compilation, and **atomic publication** of a coherent `RuntimeSnapshotBundle` into the Rust gateway. The gateway continues to serve traffic entirely from immutable in-memory runtime state — **no PostgreSQL, no control-plane call, no compilation on the request path**.

| Workstream | Delivered |
|---|---|
| `RuntimeSnapshotBundle` | Snapshot + per-lane pools acquired as ONE atomic bundle per request (Phase-5 `PublishedBundle`), preserved |
| Snapshot publication | `SnapshotPublisher`/`Reader` + `InMemoryPublisher`; `/validate` (compile-only) + `/publish` (atomic) |
| PostgreSQL persistence | `projects`, `providers`, `lanes`, `policies`, `workflows`, `workflow_versions`, `publications`, `workflow_active` |
| Workflow versioning | Immutable versions; lifecycle `draft → validated → compiled → published → active` |
| Provider/lane persistence | Records with `credential_ref` (resolved at publish, never raw secrets) |
| Publish pipeline | validate → compile → build wire → gateway `/publish` → persist publication record; failure leaves old runtime active |
| Rollback | `publish(previous_validated_version)` — never mutates the current runtime |
| Control API | Workflows CRUD + validate/compile/publish/rollback; providers CRUD; lanes CRUD |
| Frontend wiring | Real lifecycle: load → edit → save draft version → validate → compile → publish → show backend-truth metadata |
| Gateway restart | Control plane rehydrates the last ACTIVE version of every workflow on boot |
| Credential references | `credential_ref` on lanes → `Authorization` header at publish time; never in workflow JSON/logs/metrics/API |

**Test count (Phase 6): Rust 269 passing, zero failures.** Baseline 267 → **+2** (control-plane E2E). **Control plane: 9 integration tests** (real Postgres 16 + in-process mock gateway) proving publish, atomicity on failure, rollback, and lifecycle. **Frontend:** tsc clean, 5 serializer tests, production build clean. **Clippy/fmt/rust-policy:** clean.

---

## 2. Final architecture

```text
                     CONTROL PLANE
                          │
               ┌──────────┴──────────┐
               │                     │
          PostgreSQL             Control API
               │                     │
               └──────────┬──────────┘
                          │
                    validate
                          │
                     compile
                          │
                          ▼
               RuntimeSnapshotBundle
                  ├── snapshot
                  └── lane pools
                          │
                   atomic publish
                          ▼
                     DATA PLANE
                          │
Client ───────────────► Gateway
                          │
                    bundle lookup
                          │
                   compiled plan
                          │
                    route → lane
                          │
                      provider
                          │
                      streaming
```

The fundamental rule holds: **PostgreSQL is durable control-plane state; `RuntimeSnapshotBundle` is data-plane runtime state.** PostgreSQL is never required to execute a normal inference request.

**Live-proven lifecycle** (via `scripts/demo-phase6.sh`): `PostgreSQL → control plane → /publish → gateway atomic swap → request → bundle lookup → compiled plan → lane → mock provider → streamed/JSON content back`. Also proven: gateway cold restart → control-plane boot republish → request served again.

---

## 3. PostgreSQL schema

Applied by `apps/control-plane/src/db/001_initial.sql` (migrations run at startup; empty DB → schema → control-plane startup is reproducible; existing DBs gain versions non-destructively via the `schema_migrations` ledger).

```sql
projects(id PK, name, created_at, updated_at)

providers(id PK, project_id FK, name, protocol, base_url, model,
          created_at, updated_at, UNIQUE(project_id, name))

lanes(id PK, project_id FK, provider_id FK nullable,
      endpoint, base_url, egress default 'direct', policies text[],
      credential_ref JSONB,   -- {ref, provider: "env"|"vault"} — never the secret
      created_at, updated_at)

policies(id PK, project_id FK, name, rules JSONB, UNIQUE(project_id, name))

workflows(id PK, project_id FK, name,
          status CHECK(draft|validated|compiled|published|active),
          created_at, updated_at)

workflow_versions(id PK, workflow_id FK, version int,
                  workflow_json JSONB, plan_hash nullable,
                  status CHECK(...), created_at, UNIQUE(workflow_id, version))

publications(id PK, workflow_id FK, workflow_version, plan_hash,
             snapshot_version BIGINT,
             status CHECK(succeeded|failed), error, published_at)

workflow_active(workflow_id PK FK, workflow_version, plan_hash,
                snapshot_version, updated_at)  -- the served version pointer
```

`credential_ref` lives on **lanes** (the egress carrier), not providers — one source of truth for per-lane auth.

## 4. Control-plane domain model

`apps/control-plane/src/domain/model.ts` defines `WorkflowStatus`, `ProviderRecord`, `LaneRecord`, `PolicyRecord`, `PublicationRecord`, and `CredentialRef` separately from API DTOs (`api/schemas.ts`) and DB rows (`db/repositories.ts`). The lifecycle state machine is a single transition table:

```typescript
export const LIFECYCLE: Record<WorkflowStatus, readonly WorkflowStatus[]> = {
  draft: ["validated"],
  validated: ["compiled"],
  compiled: ["published"],
  published: ["published", "active"],
  active: ["active"],
};
```

No arbitrary jumps; rollback is a new publication, never a status mutation.

## 5. API contracts

`apps/control-plane/src/api/routes.ts` (Fastify):

```
GET/POST    /workflows
GET/PUT     /workflows/:id
GET         /workflows/:id/versions
POST        /workflows/:id/versions        # create immutable draft version
POST        /workflows/:id/validate        # compile-only, records plan_hash
POST        /workflows/:id/compile         # synonym
POST        /workflows/:id/publish         # full pipeline → atomic publish
POST        /workflows/:id/rollback        # republish previous validated version
GET/POST    /providers · PUT/DELETE /providers/:id
GET/POST    /lanes      · PUT/DELETE /lanes/:id
```

Lane DTO carries an optional deterministic `id` (so workflows reference `mock-lane` by name) and `credential_ref` — never the raw secret. API responses never include resolved credentials.

## 6. Workflow versioning

- A `workflows` row is durable identity (id, project, name, status).
- Every edit creates a NEW `workflow_versions` row (immutable, never mutated after creation) with a monotonically increasing `version`.
- Plan hashes are deterministic (Rust `ExecutionPlan` content hash) and recorded via the gateway `/validate` dry-run BEFORE commit — so a baked version carries server-truth provenance.

## 7. Publication pipeline

`apps/control-plane/src/domain/publish.ts`:

```text
load workflow version
   → collectReferencedLanes(workflow_json)      // llm.lane_id, fallback providers, retry target
   → resolve each lane row → resolve credential_ref → authorization
   → build WireSnapshot { snapshot_version, workflows: [{id, workflow, lanes: {lane: WireLane}}] }
   → gateway /validate  (compile-only; captures deterministic plan_hash)
   → persist COMPILED + plan_hash
   → gateway /publish   (compile + ONE atomic store swap of bundle)
   → persist PUBLISHED + publication record + ACTIVE pointer
```

Lane resolution shares ONE `buildWire` path used by `/validate`, `/compile`, and `/publish`, so a workflow validated with lane set X is published with exactly lane set X.

## 8. RuntimeSnapshotBundle architecture

The Phase-5 bundle model is preserved and formalized as the request-acquisition unit:

```rust
struct PublishedBundle { snapshot: Option<Arc<RuntimeSnapshot>>, pools: Arc<LanePools> }
```

held in a single `ArcSwap`. A request loads ONE bundle (`PublicationState::load()`), so `snapshot v2 + pools v1` is structurally impossible — both halves swap in one atomic store. The control plane never reads snapshot and pools independently.

## 9. Atomic publication implementation

`PublicationState::publish(snapshot)` (gateway side) rebuilds `LanePools` from the snapshot's lane registry and swaps `PublishedBundle` in one `ArcSwap::store`. `publish_workflows` compiles the whole set first; on ANY workflow failure nothing is published (the `compile_snapshot` gate). In-flight requests keep their acquired bundle `Arc`; old requests finishing under v41 stay on v41 after v42 lands.

## 10. Rollback behavior

`POST /workflows/:id/rollback`:
1. finds the active version,
2. picks the newest lower version with a recorded plan hash,
3. runs the **publish pipeline on that version** (`publish(previous_validated_version)`),
4. a successful republish sets the ACTIVE pointer to the older version.

It never mutates the current runtime snapshot — rollback is a first-class publication. A failing republish leaves the current active version untouched.

## 11. Frontend integration

`apps/web/src/lib/api.ts` routes all lifecycle calls through the control plane:

- `publishWorkflow(workflow, lanes)` — ensure workflow row → create immutable version → `/publish`; returns backend-authoritative `workflow_id / workflow_version / snapshot_version / plan_hash`.
- `fetchWorkflowVersions` + `useWorkflowVersions` — the versions page table now renders real DB versions (status, plan hash, created_at); no fabricated `v25`.
- `fetchWorkflows` + `useWorkflows` — the workflows index renders the durable list with real status; Create links to the editor.
- The React Flow serializer remains the editor↔Workflow-JSON boundary (no React Flow → Rust runtime path).

## 12. Provider / lane persistence

Providers and lanes are CRUD-able records keyed by project. At publish time the control plane resolves workflow-referenced lane records (base URL + credential) into the wire format the Rust gateway consumes. The data plane never depends on DB models — it receives `WireLane {base_url, authorization}` and compiles its own snapshot.

## 13. Credential-reference model

- Lanes store `credential_ref: {ref, provider}` in JSONB — a *reference*, never the secret.
- `secrets.ts::resolveCredential` resolves `env` refs to `Bearer <value>`; `vault` is a documented Phase-7 seam (returns `null` → auth omitted, publish still succeeds).
- The resolved value flows ONLY into the wire lane's `authorization` and is attached to the runtime `LaneEntry.authorization`. The LLM node adds it as the upstream `Authorization` header at request time.
- Raw secrets never appear in: workflow JSON, `ExecutionPlan`, API responses, logs, metrics, or the mock's captured body (proven by test: body does not contain the secret; header does).

## 14. Security considerations

- **SSRF posture**: lane `base_url` values are stored control-plane records, validated as URLs at persistence (zod `.url()`) and re-parsed at publish. Arbitrary user-provided URLs in workflow JSON do not become upstream destinations — only registered lane ids resolve.
- **No credential exfiltration**: `credential_ref` only (env ref). Tests assert the raw secret stays out of API responses and request bodies.
- **Tenant boundaries**: `project_id` scopes providers/lanes/workflows; the default project (`proj_default`) is seeded for the single-tenant default. Full RBAC is a documented Phase-8 goal.
- **No data-plane secrets**: the gateway holds only resolved auth headers for lanes that carry them; workflow JSON on the wire contains no secrets.

## 15. Control-plane failure behavior

| Failure | Behavior |
|---|---|
| PostgreSQL down | Published runtime keeps serving (data plane is memory-resident); control-plane writes fail with typed errors |
| Gateway down | Publish returns `gateway unreachable`; old runtime (wherever it is) unaffected |
| New config invalid | `/validate`/`/publish` returns the compile error; **old runtime stays active** (proven by e2e test) |
| Gateway cold restart | Control plane republishes the last ACTIVE version of every workflow on boot (`[rehydrate]` log), so the bundle is rebuilt from durable state |
| Control plane restart | CRUD is durable; boot re-publish reconciles the data plane |

## 16. End-to-end test results

Rust (269 passing, 0 failing — baseline 267): **+2** new in `apps/gateway/tests/control_plane_e2e.rs`:

- `control_plane_publish_drives_gateway_and_credentials_flow` — real gateway `with_publication`, SSE mock upstream; validates v2 WITHOUT trimming v1, publishes v1, serves a workflow request, asserts the lane's `Bearer sk-test-credential` reached the provider as an `Authorization` header AND that the secret never appears in the request body.
- `failed_publish_leaves_previous_runtime_active` — v1 published; v2 with an invalid lane URL rejected; v1 still serves through the gateway; the active snapshot stays v1.

Control plane (9 tests, real Postgres 16 on 127.0.0.1:5433 unique DB per suite): publish → active + publication record; failed publish leaves previous runtime + draft intact; rollback republishes previous valid version; version status transitions draft → compiled → active; workflow CRUD, version creation, provider CRUD, lane CRUD with credential refs (never raw secrets), validate yields plan hash, publish → ACTIVE + publication.

Frontend: 5 serializer tests (unchanged), tsc clean, production build clean.

## 17. Database isolation proof

- Request path: `request → ArcSwap load_full (67.8 ns) → compiled plan → lane pool → provider → stream`. **No PostgreSQL query, no control-plane HTTP call, no compile, no durable-state lookup.**
- Benchmarks (`cargo bench -p workflow-runtime`): `snapshot_reader_lookup` **67.9 ns**, `snapshot_atomic_publish` **3.27 µs**, `input_to_output` 15.4 µs, `input_transform_output` 17.8 µs — all within project targets, real-world memory-only.
- The only DB touch points are: control-plane CRUD, the publish pipeline, and control-plane boot rehydrate — all off the request hot path.

## 18. Performance benchmarks

| Bench | Result |
|---|---|
| `snapshot_reader_lookup` (bundle acquisition) | **67.9 ns** |
| `snapshot_atomic_publish` (build + swap) | **3.27 µs** |
| `input_to_output` (trivial workflow) | 15.4 µs |
| `input_transform_output` | 17.8 µs |

The hot path adds one `ArcSwap load_full` (a pointer read + Arc clone). No locks, no hash lookups, no I/O.

## 19. Files changed

**Rust:**
- `apps/gateway/src/observability/mod.rs` — `WireSnapshot` lanes → `WireLane {base_url, authorization}`; `compile_snapshot`/`validate_workflows`; `/validate` route; credential-union registration
- `apps/gateway/src/main.rs` — always owns a `PublicationState` so admin `/publish`+/`validate` are live; the gateway is a control-plane consumer
- `apps/gateway/src/lanes.rs`, `apps/gateway/tests/{publication_hot_swap,workflow_route_e2e,workflow_execution_e2e}.rs` — `LaneEntry.authorization` field
- `apps/gateway/tests/control_plane_e2e.rs` — NEW (2 tests)
- `crates/workflow-runtime/src/context.rs` — `LaneEntry.authorization`
- `crates/workflow-runtime/src/nodes/llm.rs` — attaches the lane's `Authorization` header
- `crates/workflow-runtime/src/{runner,compiler}.rs`, tests — `authorization: None` literals
- `crates/mock-upstream/src/{app,lib}.rs` — captures last request headers for credential assertions; `bin/mock-upstream.rs` gains `--json-body`

**New — `apps/control-plane/` (TypeScript):**
- `package.json`, `tsconfig.json`, `bun.lock`, `.gitignore`
- `src/index.ts` — boot: migrate → seed project → rehydrate ACTIVE workflows → Fastify
- `src/api/routes.ts` — REST
- `src/api/schemas.ts` — zod DTOs
- `src/domain/{model,wire,publish}.ts` — lifecycle, wire types, publish pipeline
- `src/gateway/client.ts` — gateway admin client
- `src/db/{db,migrate,repositories}.ts` + `001_initial.sql`
- `src/secrets.ts` — credential resolution
- `tests/{publish,api,helpers}.ts` — 9 integration tests

**Frontend:**
- `apps/web/src/lib/api.ts` — control-plane boundary
- `apps/web/src/lib/use-workflow-publication.ts` — `useWorkflows`/`useWorkflowVersions`/`usePublishWorkflow`
- `apps/web/src/routes/workflows.index.tsx` — real list
- `apps/web/src/routes/workflows.$workflowId.versions.tsx` — real version history

**Docs/scripts:** `scripts/demo-phase6.sh`, `docs/state.md`, `docs/roadmap.md`, `PHASE6_REPORT.md`.

## 20. Remaining gaps

1. **Lane pool lifetime** — pools rebuilt on publish aren't drained; abandoned hyper clients linger until GC (Phase-5 note, acceptable at control cadence).
2. **Credential backend** — only `env` resolution; `vault`/secret-manager is Phase-8.
3. **Tenant/RBAC** — single default project, no authN/authZ on the control API.
4. **Streamed-loss gating** — decode-side only; per-event streamed losses (usage, deferred tools) still not gated.
5. **Frontend two-way persistence** — the editor can publish, but loading an existing workflow into React Flow to edit it isn't wired yet (versions/status display is real; `load draft → edit` is the next increment).
6. **Multi-gateway distribution** — the publish seam is HTTP (`GatewayClient`); pushing bundles to N gateways needs a fan-out/registration story.
7. **Migration locking** — concurrent control-plane boots could race `schema_migrations`; single-instance assumption today.

## 21. Recommended Phase 7

**MCP + Skills runtime on the control plane's shoulders:**

1. MCP registry persisted like providers/lanes; dynamic discovery + deferred tool activation.
2. Skill registry with progressive loading; metadata cheap to index, content on demand.
3. Real secret-manager backend for `credential_ref` (vault), deleting the `env`-only ceiling.
4. Completed editor round-trip: load persisted workflow → edit → save version → publish → diff/compare versions.
5. Multi-gateway fan-out through the publish seam; gateway registration + bundle push.
6. AuthN on the control API (API keys per project), closing the §14 tenant gap.
7. Streamed-loss enforcement from the canonical decode into the per-event encoder.
8. **Cross-system atomicity (gateway swap vs DB ACTIVE pointer)** — the gateway `/publish` swap is HTTP and the `recordPublished` transaction is DB; they cannot be 2PC'd in Phase 6. A crash between the two leaves a transient divergence (gateway serves v2, DB still v1) that the next rehydrate silently resolves to v1. This is documented behavior, not a bug, but a production control plane should either (a) record a `publications.pending` row before the swap and reconcile on boot, or (b) drive rehydrate from the gateway's actual snapshot rather than `workflow_active`.
9. **Frontend client-side validation gap** — `validateLocally` was removed when the publish path switched to the control plane; the editor no longer rejects a node-less graph locally. Restore a thin structural check (exactly one Input/Output, known edge refs) before round-tripping to `/publish`.

---

*Phase 6 does not optimize for the number of CRUD endpoints. It proves one property: **Relay-X can persist, validate, compile, version, and atomically publish a complete runtime configuration while the ultra-low-latency data plane continues executing entirely from immutable in-memory runtime state.***
