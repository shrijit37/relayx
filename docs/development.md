# DEVELOPMENT.md
> **Status:** living · **Verified:** 2026-09-16 · **Purpose:** Repository layout, local dev setup, and the CI pipeline.

## Repository layout

```text
.
├── apps/
│   ├── gateway/                # Rust data plane (Phases 1–6.5 complete)
│   ├── control-plane/          # TypeScript/Fastify control plane + PostgreSQL + models.dev sync (Phases 6/7 complete)
│   └── web/                    # React/React Flow visual editor (TanStack Start, backend-authoritative)
├── crates/
│   ├── mock-upstream/          # Configurable mock LLM for tests/benchmarks
│   ├── test-harness/           # In-process gateway+mock spawn helpers
│   ├── protocol-core/          # Canonical protocol model + 3 adapters + catalog (Phases 2/7 complete)
│   │   └── data/catalog.json   # Vendored models.dev catalog (400 models, compile-time embedded)
│   ├── workflow-schema/        # Workflow types + graph validation (Phase 4 complete)
│   └── workflow-runtime/       # Node execution engine + compiler + snapshots (Phases 4/5/6.5 complete)
├── docs/
│   ├── README.md               # Doc index + conventions (read first)
│   ├── architecture.md         # System topology and domain model
│   ├── state.md                # Current implementation state + canonical facts
│   ├── performance.md          # Performance budget and benchmarks
│   ├── testing.md              # Test strategy and coverage
│   ├── observability.md        # Metrics and logging
│   ├── development.md          # This file
│   ├── roadmap.md              # Phase 0-9 task breakdown
│   ├── protocols.md            # Protocol translation contract
│   ├── security.md             # Threat model
│   ├── workflow-ir.md          # Workflow IR design
│   ├── mcp-skills.md           # MCP/Skills discovery
│   ├── adr-*.md                # Architecture Decision Records
│   └── archive/                # Immutable historical snapshots
├── .agents/skills/             # Skill sources (`.claude/skills/` symlinks here)
├── .claude/
│   ├── settings.json           # Hooks + permissions
│   ├── hooks/
│   │   ├── check-rust-policy.sh
│   │   ├── check-rust-gates.sh
│   │   ├── ts-guard.sh
│   │   ├── shell-guard.sh
│   │   └── check-docs.sh
│   └── agents/
│       ├── hot-path-auditor.md
│       └── protocol-fidelity-reviewer.md
├── .githooks/pre-commit        # Rust policy + docs-truth gate
├── scripts/verify-docs.sh      # Documentation truth verifier
├── .github/
│   ├── workflows/ci.yml        # CI: rust, msrv, web, control-plane, docs
│   └── dependabot.yml          # weekly grouped dependency updates
├── Cargo.toml                  # Workspace root
└── rust-toolchain.toml         # stable, rustfmt + clippy
```

### Frontend (apps/web)

```bash
cd apps/web
bun install
bun run dev       # Vite dev server with TanStack Start SSR
bun run build     # Production build to .output/
bun run typecheck # tsc --noEmit
bun test          # Serializer + run-state reducer + WorkflowBuilder interaction tests
```

### Control plane (apps/control-plane)

```bash
cd apps/control-plane
bun install
bun run dev       # Fastify API on :9091 (needs Postgres on 127.0.0.1:5433)
bun test          # Integration tests (real Postgres + in-process mock gateway)
```

## Local development

### Prerequisites

- Rust stable, at least the workspace MSRV (`rust-version` in `Cargo.toml`,
  currently **1.88**) — enforced by the CI `msrv` job
- **bun 1.4.0** — the version CI pins; both `apps/*` use it for install, test, run
- Postgres 16 for the control plane on `127.0.0.1:5433` — the `RELAYX_PG_*`
  defaults in `apps/control-plane/src/db/db.ts`. There is no compose file in this
  repo: `scripts/dev.sh` prints the exact `docker run` command and exits if the
  container is not already running.
- `scripts/dev.sh` brings up mock upstream + gateway + control plane + Postgres
- `scripts/logs.sh` merges all service logs into a single color-coded stream
- `cargo-watch` (for auto-rebuild; `cargo install cargo-watch`)

No C toolchain (`cmake`, a C compiler) is required to build the workspace: the
Prometheus exporter is declared with `default-features = false`, which keeps
`rustls`/`aws-lc-sys`/`cmake` out of the dependency graph.

### Build

```bash
cargo build --all-features --workspace           # dev profile
cargo build --release --all-features --workspace # release profile
```

All profiles live in the workspace root manifest — members must not define their
own, because Cargo only honours profiles declared by the workspace root.

| Profile | Used by | Settings |
| ------- | ------- | -------- |
| `dev` | `cargo run`, `scripts/dev.sh` | workspace `opt-level = 1`, `debug = 1`; **dependencies `opt-level = 3`** |
| `release` | deployment | `lto = "thin"`, `codegen-units = 1`, `debug = 1`, `strip = "debuginfo"` |
| `bench` | `cargo bench` | inherits `release`, overriding `debug = true`, `strip = false` |

Two consequences worth knowing:

- Dependencies are compiled at `opt-level = 3` even in dev, so the gateway's proxy
  path is representative under `cargo run` — that is what makes the latency budget
  in `docs/performance.md` observable in the local stack.
- `panic = "abort"` is deliberately **not** set for release: the data plane relies
  on unwinding for per-request panic isolation. Adding it needs an ADR.

Benchmark figures are only comparable under an identical profile — record the
profile in `docs/performance.md` whenever numbers are re-measured.

### Run the gateway

```bash
# Start a mock upstream on port 8101
cargo run -p mock-upstream -- --port 8101 --mode sse --chunks 10

# Start the gateway (routes to mock on 8101)
cargo run -p relay-gateway -- --config apps/gateway/config/gateway.toml
```

### Local dev flow (frontend + backend)

The editor talks to the control plane (`:9091`); the control plane talks to the
gateway admin API (`:9090` for `/validate`, `/publish`, `/run`) for publication
and execution:

1. Start Postgres + mock upstream + gateway + control plane + web (`scripts/dev.sh`).
   The Rust services run under `cargo watch`: they build on first start and
   rebuild + restart automatically on any source change. The control plane runs
   `bun run dev` (bun's own `--watch`), and web hot-reloads via Vite, so the
   whole stack adapts to edits without restarting the script.
   Watch all services together with `scripts/logs.sh` (or filter to specific
   ones, e.g. `scripts/logs.sh gateway web`). Every service is followed live
   with a service label, locale-formatted timestamp, and severity coloring
   (errors red, warnings yellow); control-plane pino JSON is reduced to
   readable `[level] message` lines. Missing log files are polled until they
   appear, so the viewer can be started before `dev.sh`.
2. Open the editor on `:5173`.
3. **Create** a workflow in the editor (Save in `new` mode creates the durable
   workflow row, then navigates to `/workflows/<real-id>`).
4. **Load** — the editor deserializes `latest.workflow_json` from the control
   plane into the React Flow canvas.
5. **Save** — `POST /workflows/:id/versions` (immutable version).
6. **Validate** — `POST /workflows/:id/validate` → gateway `/validate` →
   deterministic plan hash; invalid workflows return real errors.
7. **Publish** — `POST /workflows/:id/publish` → atomic gateway publication;
   version becomes ACTIVE.
8. **Run** — `POST /workflows/:id/run` (control plane) → `POST /run` (gateway
   admin) → workflow runtime → provider. The response is a JSON envelope:
   `{ status:"ok", request_id, workflow_id, snapshot_version, plan_hash, output }`.
   An unpublished workflow returns a real **409** ("Workflow must be published
   before it can be run") — Run never silently executes unsaved drafts. The
   client contract is JSON; the LLM node consumes upstream provider SSE
   internally (real streaming, bounded memory) — no per-token SSE is invented
   for the run panel.

### Run tests

```bash
# All tests
cargo test --all-features --workspace

# Unit tests only
cargo test --all-features -p relay-gateway --lib

# Specific test binary
cargo test --all-features -p relay-gateway --test proxy_integration
```

### Run benchmarks

```bash
cargo bench --bench proxy_latency -p relay-gateway
```

### Lint and format

```bash
cargo fmt --check
cargo clippy --all-targets --all-features --workspace -- -D warnings
./.claude/hooks/check-rust-policy.sh --all
```

## CI pipeline

`.github/workflows/ci.yml` runs on push to `master`/`main` and all PRs:

### Rust job

1. **Rust policy check** — forbidden patterns (unwrap, expect, todo, dead_code suppression)
2. **Format check** — `cargo fmt --check`
3. **Clippy** — `cargo clippy --locked --all-targets --all-features --workspace -- -D warnings`
4. **Test** — `cargo test --locked --all-features --workspace`

Every cargo invocation passes `--locked`, so a stale `Cargo.lock` fails the job
instead of being silently re-resolved: the committed lock is the build input.

### MSRV job

1. **Check on MSRV** — `cargo +1.88.0 check --locked --all-features --workspace` on a
   pinned 1.88.0 toolchain

This is the only job that verifies the `rust-version = "1.88"` claim (inherited by
every member from `[workspace.package]`). Resolver `"3"` makes `cargo update`
refuse dependency versions that would raise the MSRV, and this job catches
everything resolution cannot see — API use, language features, `cfg` gates.
`+1.88.0` is explicit because `rust-toolchain.toml` pins `channel = "stable"`,
which overrides whatever toolchain `rustup default` points at.

### Web job

1. **Typecheck** — `tsc --noEmit` (via `bun run typecheck`)
2. **Lint (changed files only)** — `eslint` on PR-diffed `.ts`/`.tsx` files (repo-wide lint has pre-existing prettier debt; scoped to PR scope prevents false-red CI)
3. **Test** — `bun test` (workflow-serializer + run-state reducer + WorkflowBuilder interaction tests, runs in happy-dom)
4. **Build** — `vite build` (production bundle check)

### Control-plane job

1. **Install** — `bun install --frozen-lockfile`
2. **Typecheck** — `tsc --noEmit`
3. **Test** — `bun test` against a real Postgres 16 service container

The `postgres:16-alpine` service is published on host port **5433**, matching the
`RELAYX_PG_PORT` default the app and test helpers read
(`apps/control-plane/src/db/db.ts`); no env override is needed.

### Docs job

1. **Documentation truth** — `scripts/verify-docs.sh` recomputes counts from the source and fails on drift, a broken relative doc link, a snapshot left at `docs/` top level, or a living doc missing its convention header

Features:

- Actions pinned to full commit SHAs (supply-chain security)
- `permissions: contents: read` (least-privilege)
- `timeout-minutes: 30` (Rust, MSRV), `timeout-minutes: 20` (web, control-plane), `timeout-minutes: 5` (docs)
- `concurrency` group with cancel-in-progress for PRs
- Dependabot (`.github/dependabot.yml`) opens grouped weekly PRs for cargo, both
  bun apps, and GitHub Actions; every one still has to pass all five jobs

## Rust policy

Enforced by `.claude/hooks/check-rust-policy.sh` (also runs as git pre-commit hook):

**Prohibited** (in all `.rs` files):

- `#[allow(dead_code)]` / `#[expect(dead_code)]`
- `todo!()` / `unimplemented!()`
- `.unwrap()` / `.expect(...)`

**Permitted**: Narrow `#[allow(...)]` for FFI, generated code, platform-specific code.

## Versioning

Workflow, IR, protocol, and registry schemas must be versioned independently.

Breaking changes require an ADR and migration plan.

## Mock upstream

`crates/mock-upstream` supports:

| Feature         | Config                                            |
| --------------- | ------------------------------------------------- |
| JSON mode       | `--mode json --json-body '{"ok":true}'`           |
| SSE mode        | `--mode sse --chunks 10 --chunk-size 512`         |
| TTFB delay      | `--ttfb-ms 5000`                                  |
| Chunk delay     | `--chunk-delay-ms 100`                            |
| Error injection | `x-mock-error-at` / `x-mock-error-status` headers |

Counters exposed at `/stats`: `requests_served`, `connections_accepted`, `bytes_sent`.
