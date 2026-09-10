---
name: scaffold-phase
description: Execute Phase 0 foundation setup per docs/development.md — Cargo workspace, pnpm workspace, directory layout, CI, linting, .gitignore, .editorconfig
disable-model-invocation: true
---

# Scaffold Phase 0

Set up the relay-x repository foundation per `docs/development.md` and `docs/roadmap.md`.

## When to use

- User invokes `/scaffold-phase` to bootstrap the project
- This is a deliberate, user-initiated action — never auto-triggered

## Prerequisites

- `git` available
- `cargo` available (Rust toolchain)
- `pnpm` available (Node.js + pnpm)
- `gh` authenticated (for GitHub repo creation)

## Procedure

Execute each step in order. After each step, verify it succeeded before proceeding.

### Step 1: Git initialization

```bash
cd /home/shrijit/projects/relay-x
git init
```

### Step 2: `.gitignore`

Create `.gitignore` covering:

```gitignore
# Rust
/target
**/*.rs.bk

# Node
node_modules/
dist/
.next/
.turbo/

# OS
.DS_Store
Thumbs.db

# Environment
.env
.env.local
.env.*.local

# IDE
.vscode/
.idea/
*.swp
*.swo

# Claude Code
.claude/settings.local.json
```

### Step 3: Root `Cargo.toml` (Rust workspace)

```toml
[workspace]
resolver = "2"
members = [
    "crates/routing",
    "crates/lanes",
    "crates/protocols",
    "crates/streaming",
    "crates/capabilities",
    "crates/workflow-runtime",
    "apps/gateway",
]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "MIT"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
hyper = { version = "1", features = ["full"] }
tower = { version = "0.5", features = ["full"] }
axum = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

Create minimal `Cargo.toml` for each crate with `[package]` and `[dependencies]` referencing workspace deps.

### Step 4: `pnpm-workspace.yaml`

```yaml
packages:
  - "apps/control-plane"
  - "apps/web"
  - "packages/*"
```

Create minimal `package.json` for each workspace member:

- `apps/control-plane/package.json` — name `@relay-x/control-plane`, depends on `fastify`
- `apps/web/package.json` — name `@relay-x/web`, depends on `react`, `@xyflow/react`
- `packages/protocol-core/package.json` — name `@relay-x/protocol-core`
- `packages/workflow-schema/package.json` — name `@relay-x/workflow-schema`
- `packages/sdk/package.json` — name `@relay-x/sdk`
- `packages/test-fixtures/package.json` — name `@relay-x/test-fixtures`

Root `package.json` with `"private": true` and scripts for `lint`, `typecheck`, `test`.

### Step 5: Linting and formatting config

**`.editorconfig`:**
```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true

[*.rs]
indent_style = space
indent_size = 4

[*.{ts,tsx,js,json,yaml,yml,md}]
indent_style = space
indent_size = 2
```

**`rustfmt.toml`:**
```toml
edition = "2024"
max_width = 100
tab_spaces = 4
```

**`.prettierrc`:**
```json
{
  "semi": true,
  "singleQuote": true,
  "trailingComma": "all",
  "printWidth": 100,
  "tabWidth": 2
}
```

### Step 6: GitHub Actions CI

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  rust:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --check
      - run: cargo clippy -- -D warnings
      - run: cargo test

  typescript:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: pnpm/action-setup@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm
      - run: pnpm install --frozen-lockfile
      - run: pnpm lint
      - run: pnpm typecheck
      - run: pnpm test
```

### Step 7: Directory structure

Create all directories per `docs/development.md`:

```bash
mkdir -p apps/gateway/src apps/control-plane/src apps/web/src
mkdir -p packages/protocol-core/src packages/workflow-schema/src packages/sdk/src packages/test-fixtures
mkdir -p crates/{routing,lanes,protocols,streaming,capabilities,workflow-runtime}/src
mkdir -p infra/docker infra/kubernetes infra/network
mkdir -p .github/workflows
```

Add `lib.rs` / `main.rs` stubs for each Rust crate, `index.ts` stubs for each TS package.

### Step 8: Commit

```bash
git add -A
git commit -m "chore: scaffold Phase 0 foundation

- Cargo workspace with 6 crates + gateway app
- pnpm workspace with control-plane, web, 4 packages
- GitHub Actions CI (Rust + TypeScript)
- Linting: rustfmt, clippy, prettier, editorconfig
- Directory structure per docs/development.md"
```

### Step 9: Update docs

After scaffolding, update:
- `docs/state.md` — check off Phase 0 items that are now complete
- `docs/roadmap.md` — check the boxes for completed items
- `docs/state.md` automation table — confirm all components still listed

## Verification

After completion:
1. `cargo check` succeeds (workspace resolves)
2. `pnpm install` succeeds (workspaces resolve)
3. `git log --oneline` shows the scaffold commit
4. `tree -L 2 .` matches the planned layout from `docs/development.md`
