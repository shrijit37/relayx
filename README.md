# relay-x

Ultra-low-latency, visual, programmable AI gateway/orchestrator.

## What it does

relay-x sits between LLM clients and providers, providing:

- **LLM provider and endpoint routing** — route requests to Anthropic, OpenAI, and others
- **Loss-aware protocol translation** — preserve provider-native extensions
- **Per-route network/VPN lanes** — control egress paths per provider
- **Connection pooling and streaming** — zero-buffer streaming passthrough
- **MCP server/tool discovery** — dynamic capability resolution
- **Agent Skills discovery/progressive loading** — load skills on demand
- **Visual workflow authoring** — React Flow editor (Phase 5, UI complete, mock data only)
- **Compiled workflow IR/execution plan** — fast-path proxy, full workflow execution
- **Observability, policy, fallback, retries** — production-grade reliability

## Architecture

```text
Client → Gateway (Rust) → Providers (Anthropic, OpenAI, …)
              ↑
       Control Plane (TypeScript)
              ↑
       Visual Editor (React Flow)
```

- **Data plane** (`apps/gateway/`) — Rust, Tokio, Hyper, Axum
- **Control plane** (`apps/control-plane/`) — TypeScript, Fastify (Phase 3+)
- **Visual editor** (`apps/web/`) — React, React Flow (Phase 5+)

## Status

**Phase 1 complete** — high-performance HTTP proxy with streaming, timeouts, connection pooling, and observability. **Phase 2 complete** — protocol translation engine (OpenAI Chat, Anthropic Messages, OpenAI Responses). **Frontend present** — React Flow workflow editor (mock data, no backend). See [`docs/state.md`](docs/state.md) and [`CURRENT_STATE.md`](CURRENT_STATE.md).

| Metric | Target | Actual |
|--------|--------|--------|
| Simple proxy p50 overhead | < 1 ms | ~0.105 ms |
| SSE streaming overhead | low-ms | ~0.022 ms |
| Tests | — | 197 passing |

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
  web/                  React/React Flow visual editor (TanStack Start)
crates/
  mock-upstream/        Configurable mock LLM for tests
  test-harness/         In-process test spawn helpers
  protocol-core/        Canonical protocol model + 3 adapters
  workflow-schema/      Workflow definition types + validation
  workflow-runtime/     Node-based execution engine
docs/                   Architecture, ADRs, specs
```

## Development

See [docs/development.md](docs/development.md) for local dev setup, CI pipeline, and coding standards.

## License

MIT
