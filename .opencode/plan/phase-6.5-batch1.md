# Phase 6.5 Batch 1 — Security + Backend Correctness

## Scope

4 targeted fixes addressing security, protocol fidelity, router correctness, and documentation alignment.

## F1 — Security: SQL injection defense-in-depth

**Files:** `apps/control-plane/src/db/repositories.ts`

Add a strict whitelist for dynamic `UPDATE` field names in `providers.update()` and `lanes.update()`. Currently safe (callers destructure manually) but the repository API accepts `Partial<Pick<...>>` which is fragile for future callers.

**Approach:**
- Add an `ALLOWED_PROVIDER_UPDATE_FIELDS` and `ALLOWED_LANE_UPDATE_FIELDS` const
- Filter `Object.entries(data)` against the whitelist before building `fields.push(...)` 
- Unknown keys silently ignored (not error, per current behavior)
- Add regression test attempting SQL injection payloads

**Test:** `apps/control-plane/tests/sql-injection.test.ts` — attempts unknown field names, SQL syntax payloads in field names, verifies they're stripped

---

## F2 — Backend: Protocol propagation fix

**Files:**
- `crates/workflow-runtime/src/nodes/llm.rs` — `protocol_target()` + URL construction
- `apps/gateway/src/execution.rs` — verify `ExecutionContext` setup

**Current bug:** `protocol_target(_source)` ignores source and hardcodes `OpenAiChatCompletions`. URL is hardcoded to `/v1/chat/completions`.

**Fix:**
- `protocol_target(source)` returns `source.clone()` — the protocol engine already supports all 3 adapters
- Replace hardcoded URL with protocol-aware routing:
  - `OpenAiChatCompletions` → `/v1/chat/completions`
  - `AnthropicMessages` → `/v1/messages`
  - `OpenAiResponses` → `/v1/responses`
- Ensure `build_canonical_request` carries all message content (fix `extract_messages` to preserve multimodal blocks, not just text)
- Add test: round-trip a workflow with `protocol: "anthropic"` and verify the encoded wire format is Anthropic Messages, not OpenAI Chat

**Test:** `crates/workflow-runtime/tests/protocol_propagation.rs`

---

## F3 — Backend: Router correctness

**Files:** `crates/workflow-runtime/src/nodes/router.rs`, `crates/workflow-runtime/src/execution.rs`

**Current bug:** `counter.fetch_add(1, Ordering::Relaxed) % 2` — only 2 routes. `FirstMatch` and `LoadBased` are stubs returning 0.

**Fix:**
- Router receives output port count via `config` (new field or derived from execution plan)
- `RoundRobin`: `counter % output_port_count` — derive `output_port_count` from the plan/compiler at execution time
- `FirstMatch`: pass the input through; caller routes via edge conditions (this is correct already — the router just emits on `route_0`, conditions decide)
- `LoadBased`: same as `FirstMatch` for now, add `// ponytail: round-robin until load metrics exist` comment
- Fix the per-node counter issue: the `AtomicUsize` is shared across all routers in a workflow. Either pass a `route_index: HashMap<String, AtomicUsize>` or use a per-execution-state map keyed by node ID.

**Approach:**
- Add `output_ports: usize` to `RouterConfig` (schema already has `outputs` in `SchemaNode`)
- Pass `output_port_count` from `execution.rs` when constructing node state
- Replace global counter with per-node counter stored in execution state

**Test:** `crates/workflow-runtime/tests/router_tests.rs` — 1 route, 2 routes, 3+ routes, deterministic round-robin

---

## F4 — Documentation and frontend test alignment

**Files:**
- `apps/web/package.json` — add `test` script using bun (currently missing, vitest is the wrong runner)
- `docs/state.md` — align with actual codebase findings (no fabricated frontend data exists, protocol propagation bug fixed, router bug fixed)

**Changes:**
- Add `"test": "bun test"` to `apps/web/package.json`
- Update `state.md` with corrected reality: frontend is wired to real APIs, no relay-data.ts, protocol propagation now works, router supports N routes

---

## Execution order

1. F1 (SQL injection) — standalone, no dependencies
2. F2 (protocol propagation) — standalone
3. F3 (router) — needs schema change, may touch execution.rs
4. F4 (docs/tests) — after 1-3

## Verification

- `cargo test --workspace` — all existing + new tests pass
- `bun test` in `apps/control-plane/` — 12 existing + new SQL injection tests pass  
- `bun test` in `apps/web/` — 19 existing serializer/run-state tests pass
- Manual: serialize a workflow with `protocol: "anthropic"`, verify wire format in test
