# relay-x

Ultra-low-latency, visual, programmable AI gateway/orchestrator.

## What it does

relay-x sits between LLM clients and providers. Implemented today:

- **LLM provider and endpoint routing** — route requests to Anthropic, OpenAI, and others
- **Loss-aware protocol translation** — preserve provider-native extensions (OpenAI Chat, Anthropic Messages, OpenAI Responses)
- **Connection pooling and streaming** — zero-buffer streaming passthrough
- **Compiled workflow IR/execution plan** — schema validation, semantic validation, lane validation, topological compilation, fast-path classification, real node execution (LLM/Transform/Condition/Router/Fallback/Retry) with per-lane connection pools and atomic snapshot publication
- **Durable configuration & publication** — TypeScript control plane + PostgreSQL, immutable workflow versions, atomic publish pipeline (validate → compile → gateway publish), rollback, credential references, boot-time rehydration

Backend real end-to-end; frontend is backend-authoritative (no fabricated data):

- **Visual workflow authoring** — React Flow editor wired to the control plane: load persisted versions into the canvas, Save (immutable versions), Validate (real `/validate` plan hash), Publish (atomic gateway publication), and **Run** (control-plane `/workflows/:id/run` → gateway admin `/run` → workflow runtime → provider, real envelope/errors shown). Run executes only the published ACTIVE version.
- **MCP server/tool discovery** — **planned (Phase 7), not implemented** — the UI shows an honest "not available yet" state; runtime nodes return stubs
- **Agent Skills discovery/progressive loading** — **planned (Phase 7), not implemented** — same
- **Observability/policy pages** — **no telemetry backend yet** — pages render honest "not available" states; policy enforcement and multi-tenant/cross-node observability are not implemented

## Architecture

```text
Client → Gateway (Rust) → Providers (Anthropic, OpenAI, …)
              ↑
       Control Plane (TypeScript)  ← workflows, versions, providers, lanes, run passthrough
              ↑
       Visual Editor (React Flow)  ← load/save/validate/publish/run all real; no mock data
```

- **Data plane** (`apps/gateway/`) — Rust, Tokio, Hyper, Axum
- **Control plane** (`apps/control-plane/`) — TypeScript, Fastify, PostgreSQL
- **Visual editor** (`apps/web/`) — React, React Flow (TanStack Start)

## Status

**Backend: Phase 1 complete** — high-performance HTTP proxy with streaming, timeouts, connection pooling, and observability. **Phase 2 complete** — protocol translation engine (OpenAI Chat, Anthropic Messages, OpenAI Responses). **Phase 4–6 complete** — workflow schema/compiler/runtime, atomic runtime publication, control plane + PostgreSQL. **Phase 6.5 complete** — frontend/backend integration hardening: no fabricated data, backend-authoritative UI, real end-to-end Run. Workflow execution from compiled plans works end-to-end through the gateway.

**Frontend: backend-authoritative (Phase 6.5 complete).** The workflow editor loads persisted versions from the control plane, and Save/Validate/Publish/Run are all real control-plane operations. Run executes the published ACTIVE version through the gateway admin `/run` → workflow runtime → provider, with the real envelope shown in the UI; a 409 surfaces when a workflow is unpublished. All previously-fabricated data (`relay-data.ts`, inline fixtures, `Math.sin` time series) is gone — every page fetches real backend rows or displays an honest "not available yet" state (runs history, telemetry/observability, MCP/Skills/policies/secrets). See [`docs/state.md`](docs/state.md) and [`PHASE6.5_IMPLEMENTATION_REPORT.md`](docs/archive/PHASE6.5_IMPLEMENTATION_REPORT.md).

| Metric | Target | Actual |
|--------|--------|--------|
| Simple proxy p50 overhead | < 1 ms | ~0.105 ms |
| SSE streaming overhead | low-ms | ~0.022 ms |
| Rust tests | — | 294 passing |
| Frontend tests | — | 38 passing |
| Control-plane integration tests | — | 48 passing |
| Gateway `/run` integration test | — | included (publication_hot_swap) |

## Quick start

```bash
# Build
cargo build --all-features --workspace

# Run mock upstream + gateway
cargo run -p mock-upstream -- --port 8101 --mode sse
cargo run -p relay-gateway -- --config apps/gateway/config/gateway.toml

# Test
cargo test --all-features --workspace

# Benchmark
cargo bench --bench proxy_latency -p relay-gateway
```

## Project structure

```text
apps/
  gateway/              Rust data plane
  control-plane/        TypeScript/Fastify control plane + PostgreSQL
  web/                  React/React Flow visual editor (TanStack Start)
crates/
  mock-upstream/        Configurable mock LLM for tests
  test-harness/         In-process test spawn helpers
  protocol-core/        Canonical protocol model + 3 adapters
  workflow-schema/      Workflow definition types + validation
  workflow-runtime/     Node-based execution engine + compiler + snapshots
docs/                   Architecture, ADRs, specs (docs/README.md is the index)
```

## Development

See [docs/development.md](docs/development.md) for local dev setup, CI pipeline, and coding standards.

## License

MIT
