# DEVELOPMENT.md

## Repository layout

```text
.
├── apps/
│   └── gateway/                # Rust data plane (Phase 1 complete)
├── crates/
│   ├── mock-upstream/          # Configurable mock LLM for tests/benchmarks
│   └── test-harness/           # In-process gateway+mock spawn helpers
├── docs/
│   ├── architecture.md         # System topology and domain model
│   ├── state.md                # Current implementation state
│   ├── performance.md          # Performance budget and benchmarks
│   ├── testing.md              # Test strategy and coverage
│   ├── observability.md        # Metrics and logging
│   ├── development.md          # This file
│   ├── roadmap.md              # Phase 0-8 task breakdown
│   ├── protocols.md            # Protocol translation contract
│   ├── security.md             # Threat model
│   ├── workflow-ir.md          # Workflow IR design
│   ├── mcp-skills.md           # MCP/Skills discovery
│   └── adr-*.md                # Architecture Decision Records
├── .claude/
│   ├── settings.json           # Hooks + permissions
│   ├── hooks/
│   │   └── check-rust-policy.sh
│   ├── skills/
│   │   └── architecture-guard/
│   └── agents/
│       ├── hot-path-auditor.md
│       └── protocol-fidelity-reviewer.md
├── .github/workflows/ci.yml   # CI: SHA-pinned actions, clippy, test
├── Cargo.toml                  # Workspace root
└── rust-toolchain.toml         # stable, rustfmt + clippy
```

## Local development

### Prerequisites

- Rust stable (1.88+) via `rustup`
- No external services required for Phase 1

### Build

```bash
cargo build --all-features --workspace
```

### Run the gateway

```bash
# Start a mock upstream on port 8101
cargo run -p mock-upstream -- --port 8101 --mode sse --chunks 10

# Start the gateway (routes to mock on 8101)
cargo run -p relay-gateway -- --config apps/gateway/config/gateway.toml
```

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

1. **Rust policy check** — forbidden patterns (unwrap, expect, todo, dead_code suppression)
2. **Format check** — `cargo fmt --check`
3. **Clippy** — `cargo clippy -- -D warnings`
4. **Test** — `cargo test --all-features --workspace`

Features:
- Actions pinned to full commit SHAs (supply-chain security)
- `permissions: contents: read` (least-privilege)
- `timeout-minutes: 30` (hung test protection)
- `concurrency` group with cancel-in-progress for PRs

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

| Feature | Config |
|---------|--------|
| JSON mode | `--mode json --json-body '{"ok":true}'` |
| SSE mode | `--mode sse --chunks 10 --chunk-size 512` |
| TTFB delay | `--ttfb-ms 5000` |
| Chunk delay | `--chunk-delay-ms 100` |
| Error injection | `x-mock-error-at` / `x-mock-error-status` headers |

Counters exposed at `/stats`: `requests_served`, `connections_accepted`, `bytes_sent`.
