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

- [x] canonical request/event model
- [x] Anthropic adapter
- [x] OpenAI Chat Completions adapter
- [x] OpenAI Responses adapter
- [x] protocol conformance fixtures
- [x] streaming correctness (SSE parser + boundary tests)
- [x] capability matrix
- [x] performance benchmarks for translation
- [x] gateway integration (protocol translation via `ProtocolEngine`)
- [x] property tests (SSE parser + canonical roundtrip)

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

- [x] workflow schema (types + validation — `crates/workflow-schema`)
- [x] semantic validator (`Workflow::validate()`: cycles, reachability, dead-end detection)
- [ ] compiler (React Flow graph → execution IR)
- [x] execution IR (`ExecutionPlan` via `workflow-runtime`)
- [ ] fast-path classification
- [x] runtime executor (`NodeRuntime::execute` — node handlers are stubs)

## Phase 5 — Visual editor

- [x] React Flow canvas (drag-drop, edges, selection, minimap, zoom)
- [x] node library (16 node kind variants)
- [x] lane node
- [x] provider node
- [x] route node
- [x] fallback node
- [x] MCP node
- [x] Skill node
- [ ] publish/version workflow (buttons exist, no handler)
- [ ] backend wiring (all 15 pages use mock data, zero API calls)

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
