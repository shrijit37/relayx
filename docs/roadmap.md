# ROADMAP.md

## Phase 0 — Foundation

- [ ] Repository structure
- [ ] Rust workspace
- [ ] TypeScript control-plane workspace
- [ ] React Flow web app
- [x] CI
- [x] formatting/linting
- [ ] local observability

## Phase 1 — High-performance proxy

- [x] Rust HTTP server
- [x] streaming pass-through
- [x] connection pooling
- [ ] authentication middleware
- [x] metrics/tracing
- [x] benchmark harness
- [x] direct baseline comparison

## Phase 2 — Protocol layer

- [ ] canonical request/event model
- [ ] Anthropic adapter
- [ ] OpenAI Chat Completions adapter
- [ ] OpenAI Responses adapter
- [ ] protocol conformance fixtures
- [ ] streaming correctness
- [ ] capability matrix

## Phase 3 — Lanes and routing

- [ ] lane registry
- [ ] endpoint registry
- [ ] health checks
- [ ] connection pools per lane
- [ ] direct route
- [ ] proxy route
- [ ] WireGuard integration
- [ ] fallback routing

## Phase 4 — Workflow compiler

- [ ] workflow schema
- [ ] semantic validator
- [ ] compiler
- [ ] execution IR
- [ ] fast-path classification
- [ ] runtime executor

## Phase 5 — Visual editor

- [ ] React Flow canvas
- [ ] node library
- [ ] lane node
- [ ] provider node
- [ ] route node
- [ ] fallback node
- [ ] MCP node
- [ ] Skill node
- [ ] publish/version workflow

## Phase 6 — MCP and Skills

- [ ] MCP registry
- [ ] metadata index
- [ ] dynamic discovery
- [ ] deferred tool activation
- [ ] Skill registry
- [ ] progressive loading
- [ ] retrieval evaluation suite

## Phase 7 — Production security

- [ ] secret manager integration
- [ ] tenant isolation
- [ ] policy engine
- [ ] SSRF protections
- [ ] sandboxed tool workers
- [ ] audit log

## Phase 8 — Advanced routing

- [ ] latency-aware routing
- [ ] cost-aware routing
- [ ] capacity-aware routing
- [ ] provider health scoring
- [ ] adaptive capability retrieval
- [ ] workflow-level SLOs

## Explicitly defer

- arbitrary runtime plugin installation
- marketplace
- embedded code execution in gateway process
- provider support count as a vanity metric
