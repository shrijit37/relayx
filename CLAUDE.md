# CLAUDE.md — Agent Operating Contract

## Project

This repository is an **ultra-low-latency, visual, programmable AI gateway/orchestrator**. The product combines:

- LLM provider and endpoint routing
- loss-aware protocol translation
- per-route network/VPN lanes
- connection pooling and streaming
- MCP server/tool discovery
- Agent Skills discovery/progressive loading
- visual workflow authoring with React Flow
- a compiled workflow IR/execution plan
- observability, policy, fallback, retries, and health-aware routing

The frontend is an editor. The gateway data plane is the performance-critical runtime.

## Non-negotiable architectural rules

1. **Never put the visual editor in the request hot path.** React Flow edits a canonical workflow model; the runtime executes a compiled representation.
2. **Do not flatten provider protocols into a lowest-common-denominator schema.** Preserve provider-native extensions where possible.
3. **Treat streaming as first-class.** Avoid buffering complete LLM responses unless the workflow explicitly requires it.
4. **Treat tool-reference/deferred-tool semantics as first-class.** A translator that loses deferred MCP semantics is considered incorrect even if ordinary tool calling works.
5. **Control plane and data plane are separate concerns.** Configuration writes may be relatively slow; serving traffic must remain fast and independently scalable.
6. **Do not perform network/VPN setup on every request.** Lanes are pre-provisioned and reused.
7. **Prefer immutable, versioned runtime configuration snapshots.** A request should observe one coherent configuration version.
8. **No database round trip on the normal request hot path.** Runtime configuration and route state must be memory-resident or locally cached.
9. **Every protocol adapter must have conformance tests.** Translation correctness matters more than raw feature count.
10. **Never claim zero latency.** Measure and optimize gateway-added overhead separately from upstream/provider latency.
11. **Security boundaries are explicit.** Secrets, network lanes, MCP permissions, tool permissions, and workflow execution must be policy controlled.
12. **Dynamic discovery must degrade safely.** If discovery fails, the system must have deterministic fallback behavior rather than silently inventing capabilities.

## Preferred stack

### Data plane
- Rust
- Tokio
- Hyper
- Tower
- Axum where application routing is needed
- Pingora where a dedicated high-performance proxy layer is beneficial

### Control plane
- TypeScript
- Node.js
- Fastify preferred for a lean service boundary
- PostgreSQL for durable state
- Redis only where it solves a concrete coordination/cache/queue problem

### Frontend
- React
- React Flow / xyflow
- TypeScript

## Coding style

- Keep modules small and explicit.
- Prefer typed domain models over untyped JSON internally.
- Make failure behavior explicit.
- Avoid hidden global state in the data plane.
- Use structured logging and stable error codes.
- Do not add abstractions solely for future flexibility; add them when they protect a real boundary.
- Measure before micro-optimizing.

## Rust Engineering Policy

The following are prohibited unless explicitly authorized by the user:

- `#[allow(dead_code)]`
- `#![allow(dead_code)]`
- `#[expect(dead_code)]` / `#![expect(dead_code)]` (same suppression, different spelling)
- `todo!()`
- `unimplemented!()`
- `.unwrap()`
- `.expect(...)`

These two are also allowed in test files (a panicking test just fails — no prod risk).

Narrow, item-scoped `#[allow(...)]` / `#[expect(...)]` for FFI, generated code, platform-specific code, or a documented compiler false positive is permitted. Using those attributes (or `unused`) to hide dead code is not.

When the compiler reports unused or problematic code:

1. Fix the underlying issue.
2. Remove dead or obsolete code.
3. Refactor so intended functionality has a real usage.
4. Handle `Result` and `Option` explicitly.
5. Do not silence compiler warnings merely to make the build pass.
6. Do not introduce an alternate escape hatch to bypass this policy.
7. Never weaken lint configuration to avoid fixing the problem.

If an exception is genuinely required, stop and ask for explicit user authorization before introducing it.

This policy applies to all Rust source files, including `src/`, `tests/`, `benches/`, examples, binaries, workspace crates, and build scripts — except `.unwrap()` and `.expect(...)` which are allowed in test files.

Enforcement is layered: these instructions, `.claude/hooks/check-rust-policy.sh` (PostToolUse), and CI (`--all` plus `cargo clippy -- -D warnings`). There is no pre-commit hook — wire one up if a commit-time gate is wanted.

## Build, Test, and Development Commands

### Rust workspace (data plane)

```bash
cargo build --all-features --workspace    # Build everything
cargo test --all-features --workspace     # Run all Rust tests
cargo fmt --check                         # Check formatting
cargo clippy --all-targets --all-features --workspace -- -D warnings  # Lint
cargo bench --bench proxy_latency -p relay-gateway  # Benchmarks
./.claude/hooks/check-rust-policy.sh --all          # Rust policy enforcement
```

### Frontend (apps/web)

```bash
cd apps/web
bun install
bun run dev          # Vite dev server (port 5173)
bun run build        # Production build
bun run typecheck    # TypeScript type checking
bun test             # Workflow serializer + run-state + interaction tests
```

### Control plane (apps/control-plane)

```bash
cd apps/control-plane
bun install
bun run dev          # Fastify API on :9091 (requires Postgres)
bun test             # Integration tests against real Postgres
```

### Full local stack

```bash
scripts/dev.sh       # Starts Postgres, mock upstream, gateway, control plane, web
scripts/logs.sh      # Merged color-coded log viewer for all services
```

## Testing Guidelines

- **Frameworks**: Rust `cargo test` with `#[tokio::test]` for async; frontend uses `bun test` with `happy-dom` and `@testing-library/react`.
- **Test categories**: Unit, protocol conformance, streaming boundary, property-based, integration, fault injection, load/concurrency, and security authorization.
- **Run all tests**: `cargo test --all-features --workspace` (Rust) and `bun test` (frontend).
- **Conformance**: Every protocol adapter must have conformance tests. Translation correctness matters more than feature count.
- **Test helpers**: `crates/test-harness` provides `spawn_gateway()`, `spawn_json_stack()`, `spawn_sse_stack()`, and HTTP client utilities. `crates/mock-upstream` supports SSE, JSON, TTFB delays, chunk delays, and error injection.

## Commit & Pull Request Guidelines

- **Commit messages**: Use imperative mood. Prefix with a scope tag in parentheses when applicable: `fix(gateway):`, `feat(protocol-core):`, `fix(control-plane):`, `test:`, `docs:`.
- **CI must pass**: All Rust and frontend checks (policy, fmt, clippy, tests, typecheck, build) must be green before merging.
- **Keep PRs focused**: Each PR should address a single concern. Cross-boundary changes (data plane + control plane + frontend) should clearly describe the integration points.
- **Documentation sync**: When implementation differs from docs, update the documentation in the same change. Key files: `docs/state.md`, `docs/development.md`, `docs/roadmap.md`.

## Repository boundaries

Expected high-level structure (full detail in [`docs/development.md`](docs/development.md)):

```text
apps/
  gateway/              # Rust data plane — the performance-critical proxy runtime
  control-plane/        # TypeScript/Fastify API — workflows, providers, credentials, runs
  web/                  # React/React Flow visual editor (TanStack Start)
crates/
  protocol-core/        # Canonical protocol model + adapters (OpenAI, Anthropic, Responses)
  workflow-schema/      # Workflow definition types + graph validation
  workflow-runtime/     # Node execution engine + compiler + snapshots
  mock-upstream/        # Configurable mock LLM for tests and benchmarks
  test-harness/         # In-process gateway + mock spawn helpers
docs/                   # Architecture, ADRs, specs, phase reports
scripts/                # Dev stack launcher, log viewer, demo scripts
```

## Definition of done

A change is not complete when it merely compiles. For gateway-path changes, verify:

- functional correctness
- streaming correctness
- protocol fidelity
- hot-path allocation/CPU impact
- failure behavior
- security implications
- metrics/tracing
- tests for both happy and adversarial cases

## Agent behavior

Before changing architecture, read:

1. [`docs/architecture.md`](docs/architecture.md)
2. [`docs/state.md`](docs/state.md)
3. the relevant [ADRs](docs/adr-0001-stack.md)
4. the relevant protocol/performance/security spec in [`docs/`](docs/)

When implementation differs from documentation, update the documentation in the same change.
