# STATE.md — Project State

## Status

**Phase 1 COMPLETE: High-performance HTTP proxy data plane.**

Phase 1 delivers a fully functional gateway with:
- TOML config → immutable snapshot, no DB dependency
- Route matching + lane-based upstream forwarding
- HTTP/1.1 streaming proxy (zero buffering, streaming passthrough)
- Per-request timeout (`tokio::time::timeout` wrapping handler)
- Per-frame streaming timeout (`FrameTimeoutStream` — custom `Stream` impl)
- Upstream connection pooling via `hyper_util::client::legacy::Client`
- Structured tracing (JSON) + Prometheus metrics (13 metric families)
- Typed error hierarchy with JSON error responses
- Mock LLM upstream (JSON + SSE modes, configurable latency/errors, `Drop` for clean shutdown)
- 38 integration/unit tests, 4 load tests
- Criterion benchmarks: **gateway overhead ~105µs (non-streaming), ~21µs (SSE streaming)**
- CI: SHA-pinned actions, least-privilege permissions, concurrency control, 30min timeout

This document is the source of truth for current implementation state. Update it after meaningful work.

## Current decisions

| Area | Decision | Status |
|---|---|---|
| Frontend canvas | React Flow / xyflow | Decided |
| Data plane | Rust | Decided |
| Async runtime | Tokio | Decided |
| HTTP foundation | Hyper/Tower; Axum where useful | Decided |
| Proxy option | Evaluate Pingora for dedicated proxy path | Evaluate |
| Control plane | TypeScript + Fastify | Preferred |
| Persistent DB | PostgreSQL | Preferred |
| Cache/coordination | Redis only where justified | Preferred |
| Workflow runtime | Compiled IR/execution plan | Decided |
| Routing abstraction | Lane | Decided |
| MCP | Dynamic discovery + deferred loading semantics | Decided |
| Skills | Progressive discovery/loading | Decided |
| Protocol strategy | Canonical core + provider-specific extensions | Decided |
| Streaming | First-class | Decided |

For detailed rationale on each decision, see the [ADRs](./adr-0001-stack.md) and the [Decision Log](#decision-log) below.

## Decision log

Compact log of decisions. Detailed rationale belongs in ADR files.

| Decision | State |
|---|---|
| React Flow for visual graph | Accepted |
| Rust data plane | Accepted |
| TypeScript control plane | Preferred |
| Lane = provider/endpoint + network + policy + pool | Accepted |
| Compiled workflow IR | Accepted |
| Progressive MCP discovery | Accepted |
| Progressive Skill loading | Accepted |
| Canonical protocol + extensions | Accepted |
| Separate control/data planes | Accepted |
| No synchronous DB in hot path | Accepted |
| Runtime plugin installation | Deferred / restricted |

## Implementation phases

See [Roadmap](./roadmap.md) for the full implementation phase breakdown (Phase 0-8) with task-level checkboxes.

## Open questions

1. Whether Pingora should be the primary data-plane HTTP layer or only a specialized proxy component.
2. Which canonical event schema best preserves Anthropic/OpenAI/other provider semantics.
3. How much workflow execution belongs in Rust vs a separate runtime.
4. Whether MCP tool execution should occur inside the gateway or in sandboxed workers.
5. Which vector/index strategy provides the best tool/skill retrieval latency and recall.
6. How provider-specific reasoning/caching/tool-reference semantics should be represented without loss.
7. Whether Redis is needed initially or can be delayed.

## Invariants to protect

- no synchronous DB in normal proxy path
- no full response buffering for streaming
- no silent capability loss in protocol translation
- no unbounded dynamic tool exposure
- no untrusted arbitrary plugin execution in-process
- workflow version must be pinned per request

## Claude Code Automation

Current Claude Code configuration for this repository. Update this table whenever `.claude/` config changes (per the Documentation synchronization rule in AGENTS.md).

| Component | Status | Path |
|-----------|--------|------|
| settings.json (hooks + permissions) | Installed | `.claude/settings.json` |
| rust-policy hook | Installed | `.claude/hooks/check-rust-policy.sh` |
| architecture-guard skill | Installed | `.claude/skills/architecture-guard/SKILL.md` |
| scaffold-phase skill | Installed | `.claude/skills/scaffold-phase/SKILL.md` |
| protocol-fidelity-reviewer subagent | Installed | `.claude/agents/protocol-fidelity-reviewer.md` |
| hot-path-auditor subagent | Installed | `.claude/agents/hot-path-auditor.md` |

Hooks active:

- PostToolUse: architectural-rule reminder on every Edit/Write
- PostToolUse: `.claude/hooks/check-rust-policy.sh` on every Edit/Write (fails on `dead_code` suppression, `todo!`, `unimplemented!`, `.unwrap()`, `.expect()`)
- PreToolUse: block edits to `Cargo.lock` / `pnpm-lock.yaml` / `pnpm-lock.yml` / `bun.lockb` / `yarn.lock`
- PreToolUse: block edits to `.env` files
- Git pre-commit: same rust-policy checker via `.githooks/pre-commit` (`git config core.hooksPath .githooks`)

MCP servers (context7, GitHub) will be added at Phase 0 start — they require `claude mcp add`, which modifies the global Claude Code config rather than this repository.
