---
name: architecture-guard
description: Check changed files against relay-x's 12 non-negotiable architectural rules from CLAUDE.md
---

# Architecture Guard

Audit code and documentation changes against relay-x's 12 non-negotiable architectural rules.

## When to use

- After any code change in `apps/`, `crates/`, `packages/`
- Before committing protocol adapter changes
- When reviewing PRs or agent output
- User invokes `/architecture-guard` explicitly

## Procedure

### 1. Determine what changed

If the user provides file paths or arguments, use those. Otherwise, detect changes:

```bash
# If in a git repo, use the diff
git diff --name-only HEAD~1 2>/dev/null || git diff --name-only

# If not in git, ask the user which files to check
```

### 2. Load the rules

Read `CLAUDE.md` and extract the "Non-negotiable architectural rules" section. The 12 rules are:

| # | Rule | What to check |
|---|------|---------------|
| 1 | Visual editor not in request hot path | React Flow / xyflow code must never be imported or called in `apps/gateway/` or `crates/` |
| 2 | Provider-native extensions preserved | Protocol adapters must NOT flatten provider-specific fields to a lowest-common-denominator schema. Extension fields use `extensions: { provider_name: {...} }` |
| 3 | Streaming is first-class | No code may buffer complete LLM responses unless the workflow explicitly requires it. Streaming adapters must emit events incrementally |
| 4 | Tool-ref / deferred-tool semantics preserved | Adapter pipelines must distinguish `fully_loaded_tool` from `referenced/deferred_tool`. Never silently discard tool references |
| 5 | Control plane and data plane separate | `apps/control-plane/` (TypeScript) and `apps/gateway/` (Rust) must not share runtime state. Configuration writes may be slow; traffic serving must be fast |
| 6 | Lanes pre-provisioned, not per-request | Network/VPN lane setup must not happen on the request hot path. Lanes are created once and reused |
| 7 | Immutable versioned config snapshots | A request must observe one coherent configuration version. No live-mutating config reads |
| 8 | No DB round trip on hot path | `apps/gateway/` and `crates/` must not perform synchronous database queries in request-path functions |
| 9 | Conformance tests required per adapter | Every protocol adapter in `crates/protocols/` must have a corresponding test file in `packages/test-fixtures/` |
| 10 | Never claim zero latency | No comments, docs, or metrics may assert zero gateway-added overhead. Measure and report separately from upstream latency |
| 11 | Security boundaries explicit | Secrets, network lanes, MCP permissions, tool permissions, and workflow execution must be policy-controlled. No implicit trust |
| 12 | Discovery degrades safely | If MCP/tool/skill discovery fails, the system must have deterministic fallback behavior, not silently invent capabilities |

### 3. Check each rule

For each changed file, evaluate every applicable rule. Classify each as:

- **pass** — no violation found
- **warn** — potential concern, needs human review
- **fail** — clear violation of a non-negotiable rule

Focus on:
- **Rule 1**: Grep for `react`, `@xyflow`, `ReactFlow` imports in gateway/crate code
- **Rule 2**: Check adapter files for fields being dropped or flattened during translation
- **Rule 3**: Look for `.await` after a full response read before emitting, or `collect()` on streaming bodies
- **Rule 4**: Search for `tool` handling that drops references or converts deferred → loaded without explicit opt-in
- **Rule 5**: Check for cross-plane imports (Rust importing TS types, or vice versa at runtime)
- **Rule 6**: Look for network setup, WireGuard, VPN connect in request handler functions
- **Rule 7**: Search for `Mutex<RwLock<Config>>` or live-updating config in request path
- **Rule 8**: Grep for `sqlx`, `postgres`, `redis`, `pg` calls inside `crates/` request handlers
- **Rule 9**: Verify every `crates/protocols/src/*/` file has a matching test in `packages/test-fixtures/`
- **Rule 10**: Search for "zero latency", "zero overhead", "no overhead" in comments/docs
- **Rule 11**: Check for hardcoded secrets, missing auth middleware, or policy-less tool execution
- **Rule 12**: Look for `unwrap()`, `expect()`, or missing `Option`/`Result` on discovery results

### 4. Output report

Format as a structured report:

```
## Architecture Guard Report

**Files checked**: N
**Rules evaluated**: 12
**Pass**: X | **Warn**: Y | **Fail**: Z

### Findings

| # | Rule | Status | File | Line | Evidence |
|---|------|--------|------|------|----------|
| 1 | R8: No DB on hot path | fail | crates/routing/src/handler.rs | 42 | `pool.execute(...)` inside request handler |
| 2 | R3: Streaming first-class | warn | crates/protocols/src/openai.rs | 87 | `body.collect().await` before emission |

### Blocking violations

[List any fail items — these must be fixed before the change can proceed]

### Recommendations

[Specific fixes for each warn/fail item]
```

Exit with code 1 if any **fail** findings exist. Exit 0 otherwise.
