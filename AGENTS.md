# AGENTS.md — Repository Agent Guide

## Mission

Build a production-grade AI gateway that makes complex routing, networking, protocol translation, MCP, skills, and workflow execution composable without imposing meaningful gateway overhead on the normal LLM request path.

## First principles

### 1. Fast path first

The common request should look roughly like:

```text
request
  -> authenticate
  -> resolve immutable config snapshot
  -> select lane
  -> acquire pooled upstream connection
  -> translate minimally
  -> stream
```

Avoid unnecessary:

- database access
- filesystem access
- discovery calls
- process spawning
- dynamic code loading
- JSON encode/decode cycles
- full-body buffering
- lock contention

### 2. Control plane vs data plane

Control plane:
- creates providers/endpoints/lanes
- stores workflow definitions
- manages credentials and policy
- indexes MCP/skills
- compiles workflow versions
- exposes admin APIs

Data plane:
- serves LLM traffic
- executes compiled plans
- handles streaming
- applies policy
- selects endpoints/lanes
- performs protocol adaptation
- executes permitted tools/connectors

### 3. Workflow graph is source, not runtime

React Flow state must compile into a canonical, validated IR. The data plane should consume the compiled plan, not inspect UI-specific node state.

### 4. Preserve semantics

Translation layers must preserve, where supported:

- messages/content blocks
- roles
- tool calls/results
- tool IDs
- streaming events
- structured outputs
- reasoning/thought signatures where applicable
- citations/annotations
- cache hints
- deferred tool references
- provider-specific extensions

If exact translation is impossible, report capability loss explicitly.

## Common implementation tasks

### Adding a provider

1. Define capability matrix.
2. Add protocol adapter.
3. Add request/response streaming translator.
4. Add provider-specific extension preservation.
5. Add conformance fixtures.
6. Add health-check strategy.
7. Add lane configuration.
8. Add observability fields.
9. Benchmark against direct provider access.

### Adding a network lane

1. Define lane identity.
2. Attach endpoint and network route.
3. Pre-establish/reuse connection pools.
4. Add health signals.
5. Add routing policy.
6. Add failure and failover semantics.
7. Verify secret isolation.
8. Verify DNS and egress behavior.

### Adding MCP discovery

1. Index lightweight metadata.
2. Retrieve candidates.
3. Apply policy filters.
4. Load schemas only when required.
5. Preserve deferred-tool semantics.
6. Cache stable metadata.
7. Record retrieval misses for evaluation.

### Adding a Skill

Use progressive disclosure:

```text
metadata -> skill instructions -> references/scripts/resources
```

A Skill is procedural guidance/capability context, not a replacement for an executable tool.

## Testing expectations

All major changes should add tests in the most local package plus integration tests when crossing a boundary.

Important test categories:

- unit
- protocol conformance
- streaming
- property-based translation tests
- fault injection
- load/concurrency
- network lane isolation
- security authorization
- workflow compilation determinism
- retrieval precision/recall

## Performance expectations

Track:

- gateway p50/p95/p99 overhead
- time to first byte/event
- streaming throughput
- connection reuse ratio
- allocations/request
- CPU/request
- memory under concurrency
- translation cost
- discovery cache hit rate
- retrieval miss rate

Never optimize based on intuition alone when a benchmark can isolate the bottleneck.

## Rust engineering policy

Forbidden unless the user explicitly authorizes an exception:

- `#[allow(dead_code)]` / `#![allow(dead_code)]` / `#[expect(dead_code)]`
- `todo!()` / `unimplemented!()`
- `.unwrap()` / `.expect(...)`

Do not hide dead code with `unused` allows. Other narrowly scoped `#[allow(...)]` (FFI, generated code, false positives) is allowed. Fix the underlying issue; do not weaken lints or invent a new escape hatch.

The checker is `.claude/hooks/check-rust-policy.sh`. CI runs it with `--all`.

## Documentation synchronization

Keep the documentation in realtime sync with the actual repository state. After every meaningful change:

1. **Files created or removed** — update `docs/state.md` (implementation phases) and, if the repository layout changed, `docs/development.md`.
2. **Roadmap feature completed** — check the corresponding box in `docs/roadmap.md`.
3. **Architectural decision changed or added** — update the "Current decisions" table in `docs/state.md` and create or append an ADR in `docs/adr-*.md`.
4. **Build / test / lint setup changed** — update `docs/development.md`.
5. **Claude Code config changed** (hooks, skills, agents, MCP servers) — update the "Claude Code Automation" table in `docs/state.md`.
6. **Stack or framework choice changed** — update `docs/adr-0001-stack.md` and the relevant spec docs.

Never leave documentation describing a state that no longer matches the codebase. When implementation differs from documentation, update the documentation in the same change.

## Do not do

- Do not make every node type a remote service.
- Do not introduce a queue into synchronous LLM streaming without a concrete need.
- Do not persist every event synchronously before returning it to the client.
- Do not force all providers into one lowest-common-denominator protocol.
- Do not allow arbitrary MCP/tool execution without policy boundaries.
- Do not install arbitrary runtime plugins from the internet on behalf of an agent without explicit sandbox/security design.
- Do not silence `dead_code` or use `todo!` / `unimplemented!` / `.unwrap()` / `.expect()` to make a Rust build pass.
