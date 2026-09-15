# Repository Guidelines

## Project Overview

relay-x is an ultra-low-latency, visual, programmable AI gateway/orchestrator. It routes LLM traffic between clients and providers (Anthropic, OpenAI), performs loss-aware protocol translation, executes compiled workflows, and provides a visual editor built on React Flow.

The architecture is split into a **data plane** (Rust, hot path) and a **control plane** (TypeScript, configuration).

## Project Structure

```text
apps/
  gateway/              Rust data plane — the performance-critical proxy runtime
  control-plane/        TypeScript/Fastify API — workflows, providers, credentials, runs
  web/                  React/React Flow visual editor (TanStack Start)
crates/
  protocol-core/        Canonical protocol model + adapters (OpenAI, Anthropic, Responses)
  workflow-schema/      Workflow definition types + graph validation
  workflow-runtime/     Node execution engine + compiler + snapshots
  mock-upstream/        Configurable mock LLM for tests and benchmarks
  test-harness/         In-process gateway + mock spawn helpers
docs/                   Architecture, ADRs, specs, phase reports
scripts/                Dev stack launcher, log viewer, demo scripts
```

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

## Coding Style

- **Rust**: Stable 1.88+, edition 2024, `rustfmt` with 100-char max width. No `.unwrap()`, `.expect()`, `todo!()`, `unimplemented!()`, or `#[allow(dead_code)]` in production code (`.unwrap()` and `.expect()` are allowed in tests). See `rustfmt.toml` and the full policy below.
- **TypeScript**: 4-space indentation, LF line endings. Frontend uses Bun for package management and testing.
- **Formatting**: EditorConfig is configured (`.editorconfig`). Spaces, 4-space indent for code, 2-space for TOML/YAML/JSON.
- **Modules**: Keep them small and explicit. Prefer typed domain models over untyped JSON. Make failure behavior explicit. Avoid hidden global state in the data plane.

### Rust engineering policy

The following are **prohibited** in all `.rs` files (except tests for `.unwrap()` and `.expect()`):

- `#[allow(dead_code)]` / `#[expect(dead_code)]`
- `todo!()` / `unimplemented!()`
- `.unwrap()` / `.expect(...)`

Fix the underlying issue instead. Narrow, item-scoped `#[allow(...)]` for FFI, generated code, or documented false positives is permitted. Enforcement runs via `.claude/hooks/check-rust-policy.sh` and CI.

## Testing Guidelines

- **Frameworks**: Rust `cargo test` with `#[tokio::test]` for async; frontend uses `bun test` with `happy-dom` and `@testing-library/react`.
- **Test categories**: Unit, protocol conformance, streaming boundary, property-based, integration, fault injection, load/concurrency, and security authorization.
- **Run all tests**: `cargo test --all-features --workspace` (Rust) and `bun test` (frontend).
- **CI runs on every PR**: Rust policy check, format, clippy, tests; frontend typecheck, lint (changed files only), tests, build.
- **Conformance**: Every protocol adapter must have conformance tests. Translation correctness matters more than feature count.
- **Test helpers**: `crates/test-harness` provides `spawn_gateway()`, `spawn_json_stack()`, `spawn_sse_stack()`, and HTTP client utilities. `crates/mock-upstream` supports SSE, JSON, TTFB delays, chunk delays, and error injection.

## Commit & Pull Request Guidelines

- **Commit messages**: Use imperative mood. Prefix with a scope tag in parentheses when applicable: `fix(gateway):`, `feat(protocol-core):`, `fix(control-plane):`, `test:`, `docs:`.
- **CI must pass**: All Rust and frontend checks (policy, fmt, clippy, tests, typecheck, build) must be green before merging.
- **Keep PRs focused**: Each PR should address a single concern. Cross-boundary changes (data plane + control plane + frontend) should clearly describe the integration points.
- **Documentation sync**: When implementation differs from docs, update the documentation in the same change. Key files: `docs/state.md`, `docs/development.md`, `docs/roadmap.md`.
- **Definition of done**: A change is not complete when it merely compiles. For gateway-path changes, verify functional correctness, streaming correctness, protocol fidelity, hot-path performance, failure behavior, security implications, and tests for happy and adversarial cases.

## Architecture Essentials

- **Control plane vs data plane**: Control plane is slow path (configuration, storage). Data plane is hot path (serving traffic). Never add a database round trip to the hot path.
- **Streaming is first-class**: Do not buffer complete LLM responses unless the workflow explicitly requires it.
- **Workflow graph is source, not runtime**: React Flow state compiles to a canonical IR. The data plane executes the compiled plan, not UI-specific node state.
- **Preserve provider semantics**: Do not flatten provider protocols into a lowest-common-denominator schema. Preserve provider-native extensions.
- **Security boundaries are explicit**: Secrets, network lanes, MCP permissions, tool permissions, and workflow execution must be policy-controlled.
- **Full architecture docs**: Read `docs/architecture.md`, `docs/state.md`, and `docs/adr-*.md` before making architectural changes.
