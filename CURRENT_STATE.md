# relay-x Current State

**Audit date:** 2026-09-11  
**Test run:** 197 tests passing, 0 failing  
**Branch:** `main` (with uncommitted working-tree changes in `apps/web/` and a trivial comment append in `apps/gateway/src/server/mod.rs`)

---

## Executive Summary

relay-x is an ultra-low-latency visual AI gateway/orchestrator. The **Rust data plane** (HTTP proxy + protocol translation engine) is real and functional. The **frontend** is a polished React/React Flow UI rendered entirely from static mock data — it has zero backend integration. The **control plane does not exist**. There is no database, no Redis, no Docker/Kubernetes configuration, no authentication, no network lane implementation, no MCP runtime, and no Skills runtime.

What works end-to-end: a Rust HTTP proxy that forwards LLM API requests to upstream providers, with real SSE streaming, real connection pooling, and optional event-by-event protocol translation between OpenAI Chat Completions ↔ Anthropic Messages ↔ OpenAI Responses via a typed canonical model. 197 tests pass. Gateway overhead is ~105 µs.

---

## Repository Inventory

```text
relay-x/
├── apps/
│   ├── gateway/              # Rust data plane (IMPLEMENTED)
│   └── web/                  # React frontend (MOCK-ONLY UI)
├── crates/
│   ├── mock-upstream/        # Configurable mock LLM for tests (IMPLEMENTED)
│   ├── test-harness/         # In-process test spawn helpers (IMPLEMENTED)
│   ├── protocol-core/        # Canonical model + 3 adapters (IMPLEMENTED)
│   ├── workflow-schema/      # Workflow definition types (IMPLEMENTED)
│   └── workflow-runtime/     # Node-based execution engine (STUB)
├── docs/                     # 15 documentation files (comprehensive)
├── .claude/                  # Claude Code config (hooks, agents, skills)
└── .github/workflows/ci.yml  # CI pipeline (functional)
```

**Directories that DO NOT EXIST** (despite being mentioned in architecture docs):
- `apps/control-plane/` — no TypeScript control plane
- `packages/` — no protocol-core/workflow-schema/sdk packages (they're under `crates/`)
- `infra/` — no Docker, Kubernetes, or network config
- `scripts/` — no utility scripts
- `services/` — no separate service definitions
- `database/` — no schema, migrations, or seed data
- `config/` — no environment configs beyond the single `gateway.toml`

---

## Services

### 1. relay-gateway (Rust data plane)

| Property | Value |
|---|---|
| Location | `apps/gateway/` |
| Language | Rust (edition 2024) |
| Framework | Axum 0.8 + Hyper 1.11 + Tokio |
| Entrypoint | `src/main.rs` |
| Build | `cargo build -p relay-gateway` |
| Run | `cargo run -p relay-gateway -- --config apps/gateway/config/gateway.toml` |
| Runtime | Tokio multi-thread |
| Proxy port | `127.0.0.1:8080` (TOML) / `0.0.0.0:8080` (code default) |
| Admin port | `127.0.0.1:9090` |
| Dependencies | protocol-core (path), tokio, hyper, axum, tower, tracing, metrics |
| DB | None |
| Status | **FUNCTIONAL** — serves real HTTP proxy traffic |

**What it does:**
1. Reads TOML config → compiles to immutable `ConfigSnapshot`
2. Matches incoming requests by HTTP method + path prefix → selects a lane (upstream URL)
3. Forwards requests to upstream with hop-by-hop header stripping and Host rewriting
4. Streams upstream response body to client without buffering (zero-buffer streaming)
5. Optionally translates between LLM wire protocols (OpenAI ↔ Anthropic ↔ Responses) event-by-event

**What it does NOT do:**
- No authentication
- No rate limiting
- No policy enforcement (beyond route matching and timeouts)
- No workflow execution (pure proxy)
- No MCP/tool execution
- No dynamic routing (static config only)
- No TLS (HTTP/1.1 only)
- No HTTP/2 support
- `max_concurrent` per lane is parsed but never enforced
- `frame_timeout_ms` per lane is parsed but a global 60s default is used
- `connect_timeout_ms` per lane is parsed but not applied to hyper
- `idle_timeout_ms` per lane is parsed but hardcoded to 90s for hyper

### 2. apps/web (React frontend)

| Property | Value |
|---|---|
| Location | `apps/web/` |
| Language | TypeScript |
| Framework | React 19 + TanStack Start + TanStack Router 1.170 + Vite 8.1.5 |
| Build | `bun run build` (via vite build) |
| Dev | `bun run dev` |
| Status | **SKELETON** — UI pages rendered from static mock data |

### 3. Control plane

**Does not exist.** No `apps/control-plane/` directory. No TypeScript/Fastify service. No API endpoints for workflow management, provider CRUD, lane configuration, MCP registry, or secret references.

### 4. Infrastructure

**Does not exist.** No Dockerfile, no docker-compose.yml, no Kubernetes manifests, no WireGuard configs, no nginx/traefik configs, no environment files. The system can only be started manually via `cargo run`.

---

## Frontend

### Framework

| Dependency | Version | Usage |
|---|---|---|
| React | ^19.2.0 | Active |
| TanStack Router | 1.170.18 | Active (file-based routing) |
| TanStack Start | 1.168.32 | Active (SSR server) |
| TanStack React Query | ^5.101.1 | **Unused** (wired but no queries defined) |
| Vite | 8.1.5 | Active |
| TypeScript | ^5.8.3 | Active |
| @xyflow/react | ^12.11.6 | Active (workflow editor) |
| Tailwind CSS | ^4.2.1 | Active |
| Radix UI | Multiple | Active (shadcn/ui components) |
| recharts | ^2.15.4 | Active (observability charts) |
| zod | ^3.25.76 | Installed, likely unused |
| react-hook-form | ^7.71.2 | Installed, likely unused |
| cmdk | ^1.1.1 | Active (CommandPalette) |
| sonner | ^2.0.7 | Active (toasts) |
| Lovable.dev | @lovable.dev/vite-tanstack-config ^2.20.0 | Active (build config) |

### Routes (16 files, 15 pages)

| Route | Path | Data Source | Mock? | API Calls |
|---|---|---|---|---|
| `__root.tsx` | Root layout | N/A | N/A | None |
| `index.tsx` | `/` | `relay-data.ts` | Yes | None |
| `workflows.index.tsx` | `/workflows` | `relay-data.ts` | Yes | None |
| `workflows.$workflowId.index.tsx` | `/workflows/$id` | `graph.ts` + `relay-data.ts` | Yes | None |
| `workflows.$workflowId.versions.tsx` | `/workflows/$id/versions` | `relay-data.ts` | Yes | None |
| `providers.tsx` | `/providers` | `relay-data.ts` | Yes | None |
| `lanes.tsx` | `/lanes` | `relay-data.ts` | Yes | None |
| `mcp.tsx` | `/mcp` | `relay-data.ts` | Yes | None |
| `skills.tsx` | `/skills` | `relay-data.ts` | Yes | None |
| `policies.tsx` | `/policies` | `relay-data.ts` | Yes | None |
| `secrets.tsx` | `/secrets` | Inline hardcoded | Yes | None |
| `runs.index.tsx` | `/runs` | `relay-data.ts` | Yes | None |
| `runs.$runId.tsx` | `/runs/$runId` | `relay-data.ts` + `graph.ts` | Yes | None |
| `observability.tsx` | `/observability` | `relay-data.ts` (sin/cos generated) | Yes | None |
| `health.tsx` | `/health` | Inline hardcoded | Yes | None |
| `settings.tsx` | `/settings` | `relay-data.ts` | Yes | None |

**Zero `fetch()` calls exist anywhere in the frontend.** Zero WebSocket connections. Zero server function calls. Zero tRPC calls. `@tanstack/react-query` is wired (`QueryClientProvider` in `__root.tsx`) but no queries or mutations are defined.

### Workflow Editor

The workflow editor (`/workflows/$workflowId`) is a **functional React Flow v12 canvas** with:

- **16 node kind variants** rendered through a single `RelayFlowNode` component: input, output, transform, condition, route, lane, fallback, retry, provider, endpoint, mcp, tool, skill, agent, policy, observability
- **Drag-and-drop** from `NodeLibrary` (left panel) to canvas
- **Edge connections** between nodes
- **Inspector panel** (right side) showing node configuration details — all hardcoded
- **Toolbar** with undo/redo/validate/save/versions/run-test/publish buttons — only run-test works
- **Execution simulation** — animates node states (idle → queued → running → completed) via `setTimeout` — no backend call
- **Static graph** — 14 nodes, 18 edges hardcoded in `graph.ts`
- **No persistence** — workflow state is React local state (`useNodesState`/`useEdgesState`), lost on page refresh
- **No serialization/deserialization** to/from any backend
- **No validation against the Rust workflow-schema types** — frontend has its own type system

### Existing UI Domains

| Domain | Page Exists? | Functional? | Mock? | Calls Backend? | Backend Exists? |
|---|---|---|---|---|---|
| Workflows (list) | Yes | Renders list | Yes | No | No |
| Workflow editor | Yes | React Flow canvas | Yes (static graph) | No | No |
| Providers | Yes | Renders table | Yes | No | No |
| Lanes | Yes | Renders cards | Yes | No | No |
| MCP | Yes | Renders server/tool tables | Yes | No | No |
| Skills | Yes | Renders cards | Yes | No | No |
| Policies | Yes | Renders rules | Yes | No | No |
| Secrets | Yes | Renders table | Yes (inline) | No | No |
| Runs | Yes | Renders list + waterfall | Yes | No | No |
| Observability | Yes | Renders charts (recharts) | Yes (sin/cos generated) | No | No |
| Health | Yes | Renders node statuses | Yes (inline) | No | No |
| Settings | Yes | Renders config KV | Yes | No | No |
| Versions | Yes | Renders compile stages | Yes | No | No |
| Publishing | Partial | Button exists, no handler | N/A | No | No |

---

## Control Plane

**Status: NOT_STARTED**

No TypeScript/Fastify service exists. The architecture docs describe the intended control plane:

```text
workflows | providers | lanes | MCP registry | skill registry | policies | compiler | secrets
```

None of these APIs exist. No database schema. No REST/GraphQL endpoints. No authentication middleware.

---

## Data Plane (Rust Gateway)

**Status: FUNCTIONAL**

### What exists

| Component | Module | Status | Evidence |
|---|---|---|---|
| HTTP server | `server/mod.rs` | FUNCTIONAL | Two TCP listeners (proxy + admin), Axum routing |
| Config parsing | `config/mod.rs` | FUNCTIONAL | TOML → `ConfigSnapshot`, validation, compilation |
| Route matching | `config/mod.rs` | FUNCTIONAL | Method + path prefix, first-match-wins |
| Header filtering | `transport/mod.rs` | FUNCTIONAL | Hop-by-hop stripping, Host rewriting |
| Connection pooling | `upstream/mod.rs` | FUNCTIONAL | Hyper legacy client, per-authority pool |
| Proxy handler | `proxy/mod.rs` | FUNCTIONAL | Full request forwarding, zero-buffer streaming |
| Frame timeout | `proxy/mod.rs` | FUNCTIONAL | `FrameTimeoutStream` wrapper |
| Protocol engine | `protocol.rs` | FUNCTIONAL | Event-by-event SSE translation |
| Error hierarchy | `errors/mod.rs` | FUNCTIONAL | Typed errors → HTTP status + metrics |
| Metrics | `observability/mod.rs` | PARTIAL | 13 Prometheus metric families, 4 never called |
| Health endpoints | `observability/mod.rs` | STUB | `/healthz` and `/ready` always return 200 |

### What does NOT exist

| Component | Status |
|---|---|
| Authentication middleware | NOT_STARTED |
| Rate limiting | NOT_STARTED |
| Policy engine | NOT_STARTED |
| Lane health checks | NOT_STARTED |
| Circuit breaking | NOT_STARTED |
| Retry logic | NOT_STARTED |
| TLS termination | NOT_STARTED |
| HTTP/2 | NOT_STARTED |
| Dynamic config reload | NOT_STARTED |
| Workflow execution | NOT_STARTED |
| MCP/tool execution | NOT_STARTED |
| Request size limits | NOT_STARTED |
| Concurrency limits per lane | NOT_STARTED (config parsed, never enforced) |

---

## Protocol Layer

**Status: IMPLEMENTED (3 adapters)**

### Adapters

| Protocol | Status | Evidence |
|---|---|---|
| OpenAI Chat Completions | IMPLEMENTED | Full encode/decode, streaming (~1016 lines) |
| Anthropic Messages | IMPLEMENTED | Full encode/decode, streaming, thinking blocks (~1030 lines) |
| OpenAI Responses | IMPLEMENTED | Full encode/decode, streaming, tools, reasoning (~1130 lines) |

### Feature coverage

| Feature | OpenAI Chat | Anthropic | OpenAI Responses |
|---|---|---|---|
| Request decode | ✅ | ✅ | ✅ |
| Response encode | ✅ | ✅ | ✅ |
| Streaming decode | ✅ | ✅ | ✅ |
| Streaming encode | ✅ | ✅ | ✅ |
| Tool calls/results | ✅ | ✅ | ✅ |
| System instructions | ✅ | ✅ | ✅ (as `instructions`) |
| Tool choice | ✅ | ✅ | ✅ |
| Response format | ✅ | Dropped (logged) | ✅ |
| Structured output | ✅ | Not supported (loss detected) | ✅ |
| Reasoning/thinking | Skipped | ✅ (ThinkingDelta, SignatureDelta) | ✅ (Reasoning items in response) |
| Audio content | Skipped in encode | Skipped in encode | ✅ (InputAudio, skipped in output) |
| ToolReference (deferred) | Skipped | Degrades to text annotation | N/A (no deferred refs) |
| Capability loss detection | N/A | `check_losses()` exists, NOT wired into hot path | N/A |
| SSE parser | N/A | `StreamingSseParser` (protocol-framing-level, incremental) | N/A |

### Gateway integration

The `ProtocolEngine` in `apps/gateway/src/protocol.rs` wires `protocol-core` into the proxy hot path:
- Routes declare `source_protocol`/`target_protocol`
- Requests: decode source → canonical → encode target
- Streaming: event-by-event SSE parsing → canonical → re-encode (64-entry mpsc channel)
- Non-streaming: buffer entire upstream response, decode, re-encode

---

## Providers

| Provider | Protocol | In UI? | In Control Plane? | In Gateway? | Actually Callable? |
|---|---|---|---|---|---|
| OpenAI | Chat Completions | Yes (mock) | No | Via adapter | Only if routed with correct API key in header |
| Anthropic | Messages | Yes (mock) | No | Via adapter | Only if routed with correct API key in header |
| OpenAI | Responses | No | No | Via adapter | Only if routed with correct API key in header |

No provider is pre-configured with credentials. No provider has health checking. No provider has a registered endpoint in a control plane. The gateway routes to any configured `base_url` (defaulting to mock upstreams); a provider becomes callable by pointing a lane at its real endpoint and passing credentials in request headers.

The mock upstream (`crates/mock-upstream`) simulates:
- OpenAI Chat Completions at `/v1/chat/completions` (JSON + SSE modes)
- Anthropic Messages at `/v1/messages` (deterministic response + raw SSE injection)
- Echo at `/v1/echo`, health at `/health`, stats at `/stats`
- Error injection via `X-Mock-Error-At` / `X-Mock-Error-Status` headers

---

## Network Lanes

**Status: CONCEPT ONLY (config-level static mapping)**

Lanes are defined in `gateway.toml` as static upstream URLs:

```toml
[lanes.mock-lane]
base_url = "http://127.0.0.1:8101"
```

What exists:
- `LaneConfig` struct in Rust with `base_url`, timeouts, pool settings
- Route → lane name mapping
- Lane base URL used for upstream requests

What does NOT exist:
- No lane registry service
- No per-lane connection pool isolation (one global hyper pool)
- No lane health checking
- No WireGuard / network namespace / proxy integration
- No lane failover or weighted routing
- `max_concurrent` per lane is parsed but never enforced
- `connect_timeout_ms`, `idle_timeout_ms`, `frame_timeout_ms` per lane are parsed but overridden by globals

---

## MCP

**Status: UI MOCK ONLY**

| Component | Status | Evidence |
|---|---|---|
| MCP registry | NOT_STARTED | No backend |
| MCP metadata index | NOT_STARTED | No backend |
| MCP discovery | NOT_STARTED | No backend |
| MCP tool execution | NOT_STARTED | No backend |
| MCP UI page | EXISTS | Mock data from `relay-data.ts` (5 servers, 6 tools) |
| MCP node in workflow editor | EXISTS | Visual node type, hardcoded inspector |
| MCP node in workflow runtime | STUB | Returns `{"status": "mcp_node_stub"}` |

---

## Skills

**Status: UI MOCK ONLY**

| Component | Status | Evidence |
|---|---|---|
| Skill registry | NOT_STARTED | No backend |
| Skill discovery | NOT_STARTED | No backend |
| Skill loading | NOT_STARTED | No backend |
| Skill UI page | EXISTS | Mock data from `relay-data.ts` (4 skills) |
| Skill node in workflow editor | EXISTS | Visual node type |
| Skill node in workflow runtime | STUB | Returns `{"status": "skill_node_stub"}` |

---

## Workflow Compiler

**Status: SCHEMA + RUNTIME STUB (no compiler)**

### What exists

**`crates/workflow-schema/`** — Typed workflow definition:
- `Workflow`, `Node`, `Edge`, `NodeKind` (8 kinds), `NodeConfig` (8 variants), `PortDef`, `PortType`
- `Workflow::validate()` — duplicate IDs, unknown nodes/ports, cycle detection, reachability, dead-end detection
- 12 unit tests, all passing

**`crates/workflow-runtime/`** — Execution engine skeleton:
- `ExecutionPlan::compile()` — topological sort via Kahn's algorithm
- `NodeRuntime::execute()` — runs nodes in topological order with context
- `ExecutionContext` — workflow/run IDs, cancellation token, lane registry, deadlines
- **All node implementations are stubs:**
  - LLM node: builds a `CanonicalRequest` but returns `{"status": "llm_node_stub"}`
  - MCP node: returns `{"status": "mcp_node_stub"}`
  - Skill node: returns `{"status": "skill_node_stub"}`
  - Router node: passthrough (no routing logic)
  - Transform node: passthrough only
  - Condition node: evaluates condition but doesn't route based on result
  - Input/Output nodes: passthrough

### What does NOT exist

- No workflow compiler that transforms React Flow state → execution IR
- No fast-path classification
- No workflow versioning
- No workflow publish/deploy mechanism
- No connection between frontend React Flow and `workflow-schema` types
- No workflow execution integrated into the gateway

---

## Database

**Status: NOT_STARTED**

No database technology. No schema. No migrations. No tables. No ORM. No connection handling.

Architecture docs specify PostgreSQL for durable state, but nothing is implemented.

---

## Redis / Caching

**Status: NOT_STARTED**

No Redis. No caching layer. No in-memory cache.

Architecture docs say "Redis only where justified" — no justification has been evaluated yet.

---

## Authentication & Security

**Status: NOT_STARTED**

| Component | Status |
|---|---|
| API key validation | NOT_STARTED |
| JWT/token verification | NOT_STARTED |
| mTLS | NOT_STARTED |
| Tenant isolation | NOT_STARTED |
| Secret manager | NOT_STARTED |
| SSRF protections | NOT_STARTED |
| Audit logging | NOT_STARTED |
| Rate limiting | NOT_STARTED |
| Request size limits | NOT_STARTED |
| MCP/tool permission boundaries | NOT_STARTED |

The gateway forwards `Authorization` headers as-is from client to upstream. `Proxy-Authorization` headers are stripped.

---

## Observability

**Status: PARTIAL (Rust side only)**

### Implemented (Rust gateway)

- Structured JSON tracing via `tracing-subscriber` with env-filter
- Prometheus metrics at `/metrics` (13 metric families)
- Request ID (UUID v4) in tracing spans
- Route/lane selection counters
- Request duration histogram
- Upstream connect duration, TTFB, body duration histograms
- Active requests gauge (RAII guard)
- Error category classification
- Timeout counters

### Not implemented

- 4 metric functions defined but never called: `record_bytes_in`, `record_bytes_out`, `record_upstream_body_duration`, `track_active_connection`
- No route-level label on request metrics (only lane + status)
- No connection pool metrics
- No per-protocol metrics (translation path not labeled)
- No request ID in metrics (only in traces)
- No OpenTelemetry integration
- No workflow version / route ID / tool identifiers in traces

### Frontend observability dashboard

The `/observability` page renders recharts LineChart and AreaChart with **synthetic data** generated via `Math.sin`/`Math.cos`. No real backend data.

---

## Testing

**Status: GOOD for implemented components**

### Test counts by crate

| Crate | Unit | Integration | Total | Status |
|---|---|---|---|---|
| relay-gateway | 17 | 22 | 39 | All pass |
| protocol-core | 82 | 49 | 131 | All pass |
| workflow-schema | 12 | 0 | 12 | All pass |
| workflow-runtime | 0 | 0 | 0 | **No tests** |
| mock-upstream | 4 | 0 | 4 | All pass |
| test-harness | 0 | 0 (used by others) | 0 | N/A |
| apps/web | 0 | 0 | 0 | **No tests** |
| **Total** | **115** | **71** | **197** | **All pass** |

### Test categories

| Category | Count | Quality |
|---|---|---|
| Protocol translation e2e | 22 | High — golden fixtures, round-trips, edge cases |
| SSE streaming boundary | 26 | High — byte-level splitting, malformed input, format roundtrips |
| Gateway proxy integration | 15 | High — round-trip, 404, large body, upstream errors, timeout |
| Gateway cancellation | 2 | Good — client disconnect, backpressure |
| Gateway load | 4 | Basic — 1/10/100 concurrent, connection reuse |
| Workflow schema validation | 12 | High — comprehensive graph validation |
| Mock upstream | 4 | Good — wire format, error injection |
| Benchmarks | 4 groups | Criterion, simple proxy + SSE streaming |

### Missing test categories

- Workflow runtime (zero tests)
- Workflow compiler (doesn't exist)
- Protocol translation with `check_losses()` gate
- Concurrent translation under load
- Frame timeout during streaming translation
- Frontend (zero tests)
- Security (zero tests)
- Lane isolation (no lanes)
- Network integration (no lanes)

---

## Performance

### Measured

| Metric | Target | Actual | Status |
|---|---|---|---|
| Simple proxy p50 overhead | < 1 ms | ~0.105 ms | ✅ |
| Simple proxy p95 overhead | < 2 ms | well under | ✅ |
| Simple proxy p99 overhead | < 5 ms | well under | ✅ |
| SSE streaming overhead | low-ms | ~0.022 ms | ✅ |

### Not measured

| Metric | Status |
|---|---|
| Translation path overhead | UNKNOWN (no benchmark) |
| Large body forwarding | UNKNOWN |
| Concurrent request throughput | UNKNOWN (basic load test only) |
| Memory allocation patterns | UNKNOWN |
| Connection pool behavior under load | UNKNOWN |
| Streaming translation with non-zero chunk delays | UNKNOWN |
| Memory under concurrency | UNKNOWN |
| CPU per request | UNKNOWN |

---

## Infrastructure

**Status: NOT_STARTED**

No Docker, no Docker Compose, no Kubernetes, no WireGuard configs, no reverse proxy configs, no CI beyond Rust tests.

The only infrastructure is:
- `.github/workflows/ci.yml` — Rust policy check, format, clippy, test (SHA-pinned actions, least-privilege)
- `.githooks/pre-commit` — Rust policy enforcement

---

## Frontend ↔ Backend Wiring Matrix

| Frontend feature | UI exists | Backend exists | API exists | Wired | Mocked | Status |
|---|---|---|---|---|---|---|
| Workflow list | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Workflow editor | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI + local React Flow state |
| Workflow versions | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Workflow publishing | Partial | ❌ | ❌ | ❌ | ❌ | Button exists, no handler |
| Providers | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Lanes | ✅ | Partial (TOML) | ❌ | ❌ | ✅ | Mock UI, real TOML config |
| MCP | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Skills | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Policies | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Secrets | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Runs | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| Observability | ✅ | Partial (Prometheus) | ❌ | ❌ | ✅ | Mock UI, real backend metrics |
| Health | ✅ | Partial (stub) | ❌ | ❌ | ✅ | Mock UI, always-200 backend |
| Settings | ✅ | ❌ | ❌ | ❌ | ✅ | Mock UI only |
| **Zero frontend features are wired to any backend API.** | | | | | | |

---

## Actual Runtime Flow

```text
# Current flow (what actually happens):

Client HTTP Request
  ↓
Gateway (Axum, 0.0.0.0:8080)
  ↓
proxy_handler()
  ├─ Generate UUID request_id
  ├─ ConfigSnapshot::match_route(method, path)
  │   └─ 404 if no match
  ├─ Look up LaneConfig by lane name
  ├─ build_upstream_request()
  │   ├─ Copy headers (strip hop-by-hop)
  │   ├─ Rewrite Host header
  │   └─ Pass body through (no buffer)
  ├─ forward_upstream()
  │   ├─ state.client.request() (hyper pool)
  │   ├─ Record TTFB, connect metrics
  │   └─ If route has source_protocol + target_protocol:
  │       └─ translate_proxy_request()
  │           ├─ Buffer entire client body
  │           ├─ ProtocolEngine::decode_request()
  │           ├─ ProtocolEngine::encode_request()
  │           ├─ Forward to upstream
  │           ├─ If streaming: ProtocolEngine::stream_response()
  │           │   └─ Spawn tokio task
  │           │       ├─ StreamingSseParser (incremental)
  │           │       ├─ decode_source_sse_event()
  │           │       ├─ encode_client_sse_event()
  │           │       └─ Send through 64-entry mpsc channel
  │           └─ If not streaming: buffer, decode, re-encode
  └─ Return Body to client
```

## Intended vs Actual Architecture

```text
INTENDED:                          ACTUAL:
                                   
React Flow Editor                  React Flow Editor (mock, no backend)
       ↓                                    ↓ (disconnected)
Control Plane API                  [NOTHING]
       ↓                                    ↓ (disconnected)  
Immutable Snapshot                 TOML Config → ConfigSnapshot ✓
       ↓                                    ↓
Data Plane Request Router          Route Matching ✓
       ↓                                    ↓
Lane Selection                     Lane Name Lookup ✓ (no health/failover)
       ↓                                    ↓
Protocol Translation               Protocol Translation ✓ (3 adapters)
       ↓                                    ↓
MCP/Skill Execution                [NOTHING - stubs return placeholders]
       ↓                                    ↓
Streaming Response                 Streaming Response ✓
       ↓                                    ↓
Observability                      Prometheus Metrics ✓ (partial)
                                   
Authentication                     [NOTHING]
Policy Engine                      [NOTHING]
Network Lanes (WireGuard)         [NOTHING]
Workflow Compiler                  [SCHEMA ONLY - no compiler]
```

---

## Dependency Graph

```text
Frontend (React 19 + TanStack + React Flow)
   │
   ├── (NO API calls)
   │
   ▼
[MISSING: Control Plane]
   │
   ├── [MISSING: PostgreSQL]
   ├── [MISSING: Workflow Compiler]
   ├── [MISSING: MCP Registry]
   ├── [MISSING: Skill Registry]
   │
   ▼
ConfigSnapshot (TOML, compiled at startup)
   │
   ▼
Rust Data Plane (Axum + Hyper)
   │
   ├── Config routing (✓)
   ├── Protocol Translation (protocol-core, ✓)
   │   ├── OpenAI Chat adapter (✓)
   │   ├── Anthropic Messages adapter (✓)
   │   └── OpenAI Responses adapter (stub)
   ├── Connection pooling (Hyper, ✓)
   ├── Streaming (FrameTimeoutStream + SSE parser, ✓)
   ├── Observability (Prometheus, partial)
   │
   ▼
Upstream LLM Providers (direct HTTP)
```

---

## Critical Gaps

1. **No control plane** — The frontend cannot persist workflows, manage providers, or configure lanes because no API server exists. This is the primary gap blocking frontend↔backend integration.

2. **No workflow compiler** — `workflow-schema` defines types and `workflow-runtime` can execute a plan, but there is no compiler that transforms React Flow state into an `ExecutionPlan`. The schema and runtime are disconnected.

3. **No authentication** — The gateway accepts any request and forwards it. No API key validation, no JWT, no mTLS. This is acceptable for local development but blocks any real deployment.

4. **No database** — No durable storage for workflows, versions, providers, secrets, or audit logs.

5. **No MCP/Skills runtime** — Both exist only as stubs returning placeholder JSON. No actual MCP server connection, tool discovery, or skill loading.

---

## Important Gaps

6. **Per-lane config fields ignored** — `max_concurrent`, `connect_timeout_ms`, `idle_timeout_ms`, `frame_timeout_ms` are parsed per-lane but all lanes share hardcoded globals.

7. **Connection pool not lane-isolated** — One global hyper pool for all lanes, violating ADR-0002 ("pools cannot be shared across incompatible lanes").

8. **4 observability functions never called** — `record_bytes_in`, `record_bytes_out`, `record_upstream_body_duration`, `track_active_connection` are defined but unused.

9. **Health endpoints are stubs** — `/healthz` and `/ready` always return 200 with no upstream checks.

10. **`translation_losses()` not wired** — Capability loss detection exists in `protocol-core` but is never invoked during actual request processing.

11. **Protocol fidelity gaps in translation** — ToolReference degrades to text in Anthropic adapter (logged as lossy); response_format is silently dropped when translating to Anthropic; `translation_losses()` exists but is not wired into the hot-path request processing to actually enforce capability checks.

12. **Frontend has zero tests** — No test framework configured, no test files.

13. **Workflow runtime has zero tests** — Despite having 7 node types and a topological execution engine.

---

## Future Gaps

14. No TLS termination
15. No HTTP/2
16. No WireGuard / network lane integration
17. No retry / circuit breaking
18. No dynamic config reload
19. No Docker / Kubernetes deployment
20. No OpenTelemetry integration
21. No latency/cost/capacity-aware routing
22. No workflow-level SLOs
23. No adaptive capability retrieval
24. No secret manager integration
25. No tenant isolation
26. No SSRF protections
27. No sandboxed tool workers
28. No audit logging

---

## Contradictions

| Claim (docs) | Reality (code) | Evidence | Interpretation |
|---|---|---|---|
| "Phase 1 COMPLETE" (state.md) | Gateway is functional with 39 tests | All tests pass | Accurate for Phase 1 scope |
| "Phase 2 COMPLETE" (state.md) | Protocol engine with 101 tests | All tests pass | Accurate for Phase 2 scope |
| "Ready for Phase 3" (state.md) | No lane health, no failover, no WireGuard | `upstream/mod.rs` | Phase 3 has not started |
| "Total tests: 167" (state.md) | Actually 197 (workflow-schema adds 12, workflow-runtime stubs add 0) | `cargo test` output | state.md is slightly outdated |
| OpenAI Responses "Full adapter" (state.md M2.6) | Fully implemented with 1130 lines, 7 unit tests | `openai_responses/mod.rs` (decode_request, encode_response, encode_stream_event, decode_response all working) | State.md is accurate — the adapter was completed after the Phase 1-2 report date |
| "max_concurrent per lane" (gateway.toml) | Config field parsed but never enforced | `server/mod.rs:58`, `proxy/mod.rs` (no semaphore) | Dead config field |
| "per-lane frame_timeout" (gateway.toml) | Hardcoded to 60s globally | `server/mod.rs:58` comment | Dead config field |
| Frontend is "Phase 5+" (roadmap) | Frontend already exists with 15 pages | `apps/web/` | Frontend was built early (via Lovable.dev) |
| "Control plane (Phase 3+)" (roadmap) | Does not exist | No `apps/control-plane/` directory | Correct per roadmap |
| architecture.md describes "capability runtime" in data plane | No MCP/tool execution exists | `workflow-runtime/src/nodes/mcp.rs` is a stub | Design intent, not reality |
| "12 non-negotiable rules" (CLAUDE.md) | Rules 4, 6, 8, 11, 12 have no implementation | No MCP execution, no lanes, no DB, no security, no graceful degradation | Rules are aspirational constraints |

---

## Recommended Implementation Starting Point

The single most logical first step is **building the TypeScript control plane API** (`apps/control-plane/`). The reasons:

1. The frontend has 15 fully-built pages using mock data — wiring them to real APIs requires an API server first.
2. The control plane is the bridge between the visual editor (frontend) and the data plane (gateway). Without it, the workflow-schema and workflow-runtime crates have no input.
3. The gateway's TOML config is a dead end for production use — a control plane that manages providers, lanes, and workflow versions and distributes immutable snapshots to the data plane is the architecture's central nervous system.
4. A minimal control plane (workflow CRUD, provider CRUD, lane configuration, compile-to-IR endpoint) would immediately make the existing frontend functional and unlock Phase 4 (workflow compiler) work.

The control plane should start with: Fastify + TypeScript + PostgreSQL schema for workflows/providers/lanes + REST endpoints + the workflow compiler that produces `ExecutionPlan` from `workflow_schema::Workflow`.

---

## Evidence / Source Files

| Evidence | File |
|---|---|
| Gateway proxy handler | `apps/gateway/src/proxy/mod.rs` |
| Config parsing | `apps/gateway/src/config/mod.rs` |
| Protocol engine | `apps/gateway/src/protocol.rs` |
| Protocol adapters | `crates/protocol-core/src/adapters/` |
| Canonical model | `crates/protocol-core/src/canonical.rs` |
| SSE parser | `crates/protocol-core/src/sse.rs` |
| Workflow schema | `crates/workflow-schema/src/lib.rs` |
| Workflow runtime | `crates/workflow-runtime/src/execution.rs` |
| LLM node stub | `crates/workflow-runtime/src/nodes/llm.rs` |
| MCP node stub | `crates/workflow-runtime/src/nodes/mcp.rs` |
| Skill node stub | `crates/workflow-runtime/src/nodes/skill.rs` |
| Frontend mock data | `apps/web/src/lib/relay-data.ts` |
| Workflow editor | `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx` |
| Workflow graph | `apps/web/src/components/relay/workflow/graph.ts` |
| Gateway config | `apps/gateway/config/gateway.toml` |
| CI pipeline | `.github/workflows/ci.yml` |
| All 15 docs | `docs/*.md` |

---

## Summary

```text
CURRENT SYSTEM:
  5 Rust crates (3 fully functional, 1 schema, 1 stub)
  1 web frontend (15 pages, all mock data, zero API calls)
  0 control plane services
  0 database tables
  0 wired frontend features
  15 mocked frontend features
  197 tests (all passing, 0 in frontend)
  3 protocol adapters (all functional: OpenAI Chat, Anthropic Messages, OpenAI Responses)
  0 MCP runtime connections
  0 Skills loaded
  0 real network lanes
  0 authentication mechanisms
  5 critical gaps

MOST IMPORTANT:
The control plane (TypeScript/Fastify + PostgreSQL) does not exist and is the single
largest gap in the system. It is the bridge between the 15-page frontend (which has
mock data) and the Rust data plane (which is functional). Building a minimal control
plane with workflow CRUD, provider CRUD, lane configuration, and a compiler that
produces ExecutionPlan from workflow_schema::Workflow would immediately make the
frontend functional, connect the schema/runtime crates to real data, and unlock the
path to Phase 4 (workflow compiler) and Phase 6 (MCP/Skills) work.
```
