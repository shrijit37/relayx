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
docs/                   Architecture, ADRs, specs; docs/archive/ holds snapshots
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

### Documentation

```bash
scripts/verify-docs.sh          # Fail if docs disagree with the code
scripts/verify-docs.sh --write  # Regenerate the canonical-facts block
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
- **Documentation sync (enforced)**: When implementation differs from docs, update the docs in the same change. `scripts/verify-docs.sh` fails the Claude Code hook, the git pre-commit hook, the pre-push gate, and CI when canonical counts or links drift. See [Documentation conventions](#documentation-conventions).
- **Definition of done**: A change is not complete when it merely compiles. For gateway-path changes, verify functional correctness, streaming correctness, protocol fidelity, hot-path performance, failure behavior, security implications, and tests for happy and adversarial cases.

## Documentation conventions

Full conventions and the document map: [`docs/README.md`](docs/README.md).

- **Living vs. snapshot**: everything in `docs/` except `archive/` is living and carries a `> **Status:** living · **Verified:** YYYY-MM-DD · **Purpose:** …` header. Dated reports and audits live in [`docs/archive/`](docs/archive/) and are never edited.
- **One canonical fact**: test/route counts are computed from the code and written once — the canonical-facts block in [`docs/state.md`](docs/state.md). Link to it, or quote the number and let the verifier check it.
- **ADRs are append-only**: supersede a decision, never rewrite it.
- **Enforcement**: `scripts/verify-docs.sh` (check) and `scripts/verify-docs.sh --write` (regenerate facts) run in the Claude Code hook, the git pre-commit hook, and the CI `docs` job.

### Which doc to update for a change

| Change | Update |
| --- | --- |
| Behavior, phase status, test counts | [`docs/state.md`](docs/state.md), then `scripts/verify-docs.sh --write` |
| A boundary or component relationship | [`docs/architecture.md`](docs/architecture.md) |
| Tooling, layout, CI | [`docs/development.md`](docs/development.md) |
| Test suites or categories | [`docs/testing.md`](docs/testing.md) |
| Roadmap item starts or finishes | [`docs/roadmap.md`](docs/roadmap.md) |
| Adapter or translation rule | [`docs/protocols.md`](docs/protocols.md) |
| Schema, compiler, or IR | [`docs/workflow-ir.md`](docs/workflow-ir.md) |
| Perf-sensitive path or benchmark | [`docs/performance.md`](docs/performance.md) |
| Metric, log, or trace change | [`docs/observability.md`](docs/observability.md) |
| Auth, secrets, or permissions | [`docs/security.md`](docs/security.md) |
| An architectural decision | new `docs/adr-XXXX-*.md` |

## Architecture Essentials

- **Control plane vs data plane**: Control plane is slow path (configuration, storage). Data plane is hot path (serving traffic). Never add a database round trip to the hot path.
- **Streaming is first-class**: Do not buffer complete LLM responses unless the workflow explicitly requires it.
- **Workflow graph is source, not runtime**: React Flow state compiles to a canonical IR. The data plane executes the compiled plan, not UI-specific node state.
- **Preserve provider semantics**: Do not flatten provider protocols into a lowest-common-denominator schema. Preserve provider-native extensions.
- **Security boundaries are explicit**: Secrets, network lanes, MCP permissions, tool permissions, and workflow execution must be policy-controlled.
- **Full architecture docs**: Read `docs/architecture.md`, `docs/state.md`, and `docs/adr-*.md` before making architectural changes.
