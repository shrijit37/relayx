# relay-x: Phase 1 & 2 Implementation Report

**Date:** 2026-09-10
**Repository:** `shrijit37/relay-x`
**Branch:** `feat/phase1-data-plane` → `main`
**Rust toolchain:** stable 1.98.1

---

## 1. Executive Summary

This report documents the complete implementation of Phase 1 (high-performance HTTP proxy) and Phase 2 (protocol engine) of the relay-x gateway. The system is an ultra-low-latency, visual, programmable AI gateway/orchestrator that sits between LLM clients and providers.

| Metric | Phase 1 | Phase 2 |
|--------|---------|---------|
| Source files (.rs) | 18 | 28 (+10) |
| Source lines of code | 3,282 | 7,486 (+4,204) |
| Test files | 5 | 7 (+2) |
| Total tests | 38 | 109 (+71) |
| Documentation files | 15 | 15 |
| Documentation lines | 1,751 | 1,751 |
| Workspace crates | 3 | 4 (+1) |

---

## 2. Architecture Decisions (ADRs)

### ADR-0001: Core Stack

**Decision:** Rust/Tokio for the data plane, TypeScript/Fastify for the control plane, React/React Flow for the editor, PostgreSQL for durable state.

**Rationale:** The gateway is network and streaming heavy — Rust provides predictable resource usage and an async runtime (Tokio) that handles thousands of concurrent connections efficiently. TypeScript is productive for CRUD/control-plane APIs. React Flow directly matches the graph editor requirement.

**Consequences:** Two primary backend languages; shared domain schemas need disciplined ownership; more deployment artifacts; clear separation between control and data plane.

### ADR-0002: Lane as Primary Routing Abstraction

**Decision:** Model a route as a **lane** that binds: provider/endpoint + network egress + policy + connection pool.

**Rationale:** Routing decisions cannot be expressed as model→provider only. The network path is a first-class operational constraint (VPN, geo, compliance).

**Consequences:** Routing, health, connection pooling, and network policy must all understand lane identity. Pools cannot be shared across incompatible lanes.

### ADR-0003: Preserve Protocol Semantics

**Decision:** Use a canonical internal model with explicit extension fields instead of a lowest-common-denominator translation model.

**Rationale:** Modern agent clients use provider-specific semantics (deferred tool references, streaming event types, structured outputs, reasoning metadata, cache hints). Flattening causes silent feature loss.

**Consequences:** Adapters are more complex. Capability negotiation and conformance testing become mandatory.

### ADR-0004: Separate Control and Data Planes

**Decision:** Configuration, workflow authoring, registries, secrets references, and compilation belong to the control plane. Serving traffic belongs to the data plane.

**Rationale:** Avoids database and control-plane dependencies in the latency-critical request path and permits independent scaling.

**Consequences:** Runtime config must be versioned and distributed as immutable snapshots.

---

## 3. Non-Negotiable Architectural Rules

These 12 rules govern every code change in the repository:

1. **Never put the visual editor in the request hot path.** React Flow edits a canonical workflow model; the runtime executes a compiled representation.
2. **Do not flatten provider protocols into a lowest-common-denominator schema.** Preserve provider-native extensions where possible.
3. **Treat streaming as first-class.** Avoid buffering complete LLM responses unless the workflow explicitly requires it.
4. **Treat tool-reference/deferred-tool semantics as first-class.** A translator that loses deferred MCP semantics is considered incorrect.
5. **Control plane and data plane are separate concerns.** Configuration writes may be relatively slow; serving traffic must remain fast.
6. **Do not perform network/VPN setup on every request.** Lanes are pre-provisioned and reused.
7. **Prefer immutable, versioned runtime configuration snapshots.** A request should observe one coherent configuration version.
8. **No database round trip on the normal request hot path.** Runtime configuration and route state must be memory-resident.
9. **Every protocol adapter must have conformance tests.** Translation correctness matters more than raw feature count.
10. **Never claim zero latency.** Measure and optimize gateway-added overhead separately from upstream/provider latency.
11. **Security boundaries are explicit.** Secrets, network lanes, MCP permissions, tool permissions, and workflow execution must be policy controlled.
12. **Dynamic discovery must degrade safely.** If discovery fails, the system must have deterministic fallback behavior.

---

## 4. Phase 1: High-Performance HTTP Proxy Data Plane

### 4.1 What Was Built

A complete HTTP proxy gateway that forwards requests from clients to upstream LLM providers with zero-buffer streaming, connection pooling, and observability.

### 4.2 Workspace Structure

```text
Cargo.toml                    Workspace root (resolver v2, edition 2024)
apps/
  gateway/                    The gateway binary + library crate
    Cargo.toml                Dependencies: tokio, hyper, axum, tower, serde, etc.
    src/
      lib.rs                  Library root — re-exports config, server, proxy, etc.
      main.rs                 Binary entry point — CLI (clap), config load, server start
      config/mod.rs           TOML config → immutable ConfigSnapshot (495 lines)
      server/mod.rs           GatewayServer — binds listeners, serves traffic (162 lines)
      proxy/mod.rs            Proxy handler — the hot path (308 lines)
      transport/mod.rs        Header filtering, hop-by-hop stripping (158 lines)
      upstream/mod.rs         hyper client builder (39 lines)
      errors/mod.rs           Typed error hierarchy (256 lines)
      observability/mod.rs    Tracing + Prometheus metrics (174 lines)
    benches/
      proxy_latency.rs        Criterion benchmarks (221 lines)
      harness.rs              Benchmark spawn helpers (91 lines)
    tests/
      proxy_integration.rs    15 integration tests (369 lines)
      cancellation_integration.rs  2 cancellation tests (105 lines)
      load_test.rs            4 load tests (92 lines)
    config/
      gateway.toml            Example configuration
crates/
  mock-upstream/              Configurable mock LLM upstream
    src/
      lib.rs                  MockUpstream spawn/shutdown (179 lines)
      app.rs                  Axum router for mock endpoints (135 lines)
      sse.rs                  SSE event generator (192 lines)
      bin/mock-upstream.rs    CLI binary (82 lines)
  test-harness/               In-process test helpers
    src/lib.rs                spawn_gateway, spawn_json_stack, etc. (209 lines)
docs/                         Architecture, ADRs, specs (15 files, 1,751 lines)
```

### 4.3 Key Design Decisions

#### Decision: Immutable Config Snapshot

**What:** TOML config is parsed once at startup into a `ConfigSnapshot` — an `Arc`-wrapped struct with pre-compiled routes and lanes. No config parsing on the hot path.

**Why:** Rule 7 (immutable snapshots) and Rule 8 (no DB on hot path). The snapshot is an `Arc<ConfigSnapshot>` shared across all requests. Route matching is a linear scan of pre-compiled `CompiledRoute` entries with pre-parsed `http::Method` values.

**Trade-off:** Config changes require a process restart. This is acceptable because the control plane (Phase 3+) will manage config distribution, and gateway instances are horizontally scalable.

#### Decision: Zero-Buffer Streaming

**What:** The proxy handler forwards the upstream response body as a `Body::from_stream()` — no buffering of the complete response. Each TCP frame from upstream is forwarded to the client as-is.

**Why:** Rule 3 (streaming is first-class). LLM responses can be 100KB+ and arrive over 30+ seconds. Buffering would add latency and memory pressure.

**Implementation:** The `FrameTimeoutStream` wrapper monitors per-frame arrivals. If no frame arrives within the configured timeout (default 60s), the stream terminates with an error. This prevents stalled upstreams from holding connections indefinitely.

**Trade-off:** Once response headers are sent, errors in the body stream cannot be communicated as HTTP status codes — they terminate the connection. This is the standard behavior for streaming HTTP responses.

#### Decision: Axum for Application Routing, Hyper for Connection Pooling

**What:** Use `axum` for HTTP request routing and handler dispatch (application layer), but `hyper_util::client::legacy::Client` for the upstream connection pool (transport layer).

**Why:** Axum provides ergonomic request routing, extraction, and middleware. Hyper's legacy client provides a production-grade connection pool with keep-alive, idle timeout, and per-host connection limits. Mixing them gives the best of both.

**Trade-off:** Two HTTP layers (axum + hyper) add a small amount of abstraction overhead. The measured overhead (~105µs) is well within budget.

#### Decision: Path-Prefix Route Matching

**What:** Routes are matched by HTTP method + path prefix (e.g., `POST /v1/chat/completions`). First match wins.

**Why:** Simple, predictable, and sufficient for the initial gateway. LLM APIs use well-defined paths (`/v1/chat/completions`, `/v1/messages`). More sophisticated routing (regex, headers, model-based) arrives in Phase 3+.

**Trade-off:** Cannot route based on request body content (e.g., model name). This is acceptable because the control plane will handle model-based routing in Phase 3+.

#### Decision: Hop-by-Hop Header Stripping

**What:** The transport layer strips hop-by-hop headers (`Connection`, `Host`, `TE`, `Transfer-Encoding`, `Upgrade`, `Proxy-*`, `Connection-*`) from both client→upstream and upstream→client directions. `Host` is re-derived from the lane's base URL.

**Why:** RFC 9110 §7.6.1 requires hop-by-hop headers to be consumed by a single transport hop. Forwarding them would cause incorrect behavior (e.g., `Connection: keep-alive` being forwarded to a different server).

#### Decision: Per-Request + Per-Frame Timeouts

**What:** Two timeout layers: (1) `tokio::time::timeout` wrapping the entire handler (default 120s), and (2) `FrameTimeoutStream` monitoring per-frame arrivals (default 60s).

**Why:** The per-request timeout prevents slow requests from consuming resources indefinitely. The per-frame timeout specifically catches stalled streaming connections where the upstream has stopped sending data but hasn't closed the connection.

**Trade-off:** Two separate timeout mechanisms add complexity. But they address different failure modes: the per-request timeout catches slow TTFB; the per-frame timeout catches mid-stream stalls.

#### Decision: Typed Error Hierarchy

**What:** `GatewayError` enum with 7 variants, each mapping to a specific HTTP status code and error category for metrics.

**Why:** Rule 11 (security boundaries) and observability. Typed errors prevent internal Rust details from leaking to clients. The `IntoResponse` impl serializes errors as structured JSON with status, error type, and message.

**Variants:** `InvalidRequest` (400), `UpstreamConnection` (502), `UpstreamTimeout` (504), `UpstreamHttp` (upstream status), `UpstreamProtocol` (502), `ClientCancelled` (499), `Internal` (500).

#### Decision: Mock Upstream for Testing

**What:** A configurable mock LLM server (`crates/mock-upstream`) that supports JSON and SSE modes, configurable latency, chunk delays, error injection at specific chunk indices, and connection resets.

**Why:** Integration tests need a controlled upstream to verify proxy behavior. The mock's `MockState` counters (`requests_served`, `connections_accepted`, `bytes_sent`) allow tests to assert behavior without timing-dependent assertions.

**Design:** The mock uses `tokio::sync::oneshot` for clean shutdown via `Drop`. SSE events are generated using `futures_util::stream::unfold` — a lazy, streaming iterator that respects chunk delays.

#### Decision: Criterion Benchmarks with Direct Baseline

**What:** Benchmarks compare three scenarios: (A) client→mock direct, (B) client→gateway→mock, (C) real provider. The difference between A and B isolates gateway overhead.

**Why:** Rule 10 (never claim zero latency). Measuring overhead in isolation prevents conflating gateway latency with network/provider latency.

**Results:**
| Metric | Target | Actual |
|--------|--------|--------|
| Simple proxy p50 overhead | < 1 ms | ~0.105 ms |
| SSE streaming overhead | low-ms | ~0.022 ms |

### 4.4 Observability

**Metrics (13 families):**
- `relayx_request_total` (counter, labels: status, lane)
- `relayx_request_duration_ms` (histogram, labels: status, lane)
- `relayx_upstream_connect_ms` (histogram, labels: lane)
- `relayx_upstream_ttfb_ms` (histogram, labels: lane)
- `relayx_upstream_body_duration_ms` (histogram, labels: lane)
- `relayx_active_requests` (gauge, labels: lane) — RAII guard
- `relayx_active_connections` (gauge, labels: lane) — RAII guard
- `relayx_bytes_in` / `relayx_bytes_out` (counters)
- `relayx_errors_total` (counter, labels: category)
- `relayx_timeout_total` (counter, labels: lane)
- `relayx_route_selected_total` / `relayx_lane_selected_total` (counters)

**Admin endpoints:** `/healthz` (liveness), `/ready` (readiness), `/metrics` (Prometheus format).

**Logging:** Structured JSON via `tracing-subscriber` with `EnvFilter` (default: `relay_x=info,tower_http=info`).

### 4.5 Security

- No secrets in config files — credentials are injected via environment.
- Hop-by-hop header stripping prevents header injection.
- `Host` header is re-derived from the lane URL, not forwarded from client.
- `Proxy-*` headers are stripped.
- `--remote-allow-origins=*` was explicitly removed from Chrome CDP (documented in system CLAUDE.md).
- Error responses do not leak internal paths or stack traces.

### 4.6 Test Coverage

| Category | Count | What's tested |
|----------|-------|---------------|
| Unit tests | 13 | Config parsing/validation/compilation, error status codes, transport header filtering, upstream client builder, server healthz/ready, proxy request rewriting |
| Integration tests | 15 | JSON round-trip, query string, 404, large body, SSE passthrough, upstream errors (4xx/5xx), connection refused, timeout, slow chunks, timeout during streaming, concurrent disconnect, healthz/ready, metrics |
| Cancellation tests | 2 | Client disconnect drops upstream task, slow client receives backpressure |
| Load tests | 4 | 1/10/100 concurrent requests, connection reuse (50 sequential) |
| Mock upstream tests | 4 | SSE wire format, event builder, response builder, error-at-zero |
| **Total** | **38** | |

### 4.7 CI/CD

```yaml
# .github/workflows/ci.yml
- Rust policy check (.claude/hooks/check-rust-policy.sh --all)
- Format check (cargo fmt --check)
- Clippy (cargo clippy -- -D warnings)
- Tests (cargo test --all-features --workspace)
```

SHA-pinned actions, least-privilege permissions (`contents: read`), concurrency control, 30-minute timeout.

---

## 5. Phase 2: Protocol Engine

### 5.1 What Was Built

A typed canonical protocol model and adapters that translate between OpenAI Chat Completions, Anthropic Messages, and the internal representation. The engine is designed so additional protocols can be added without rewriting the gateway core.

### 5.2 Workspace Addition

```text
crates/
  protocol-core/              Canonical protocol model + adapters
    Cargo.toml                Dependencies: serde, serde_json, bytes, http, thiserror, tracing
    src/
      lib.rs                  Library root (20 lines)
      canonical.rs            Typed canonical model (635 lines)
      error.rs                ProtocolEngineError (130 lines)
      sse.rs                  StreamingSseParser (345 lines)
      adapters/
        mod.rs                Adapter module root (9 lines)
        openai_chat/mod.rs    OpenAI Chat Completions adapter (1,016 lines)
        anthropic_messages/mod.rs  Anthropic Messages adapter (1,030 lines)
        openai_responses/mod.rs    OpenAI Responses stub (77 lines)
    tests/
      translation_e2e.rs      End-to-end translation tests (623 lines)
      streaming_boundary.rs   SSE parser boundary tests (280 lines)
```

### 5.3 Key Design Decisions

#### Decision: Typed Canonical Model (Not `serde_json::Value`)

**What:** The canonical model uses strongly typed Rust enums and structs — `ContentBlock`, `Message`, `CanonicalRequest`, `CanonicalResponse`, `ToolUseBlock`, `ToolResultBlock`, `ToolReference`, `Usage`, `FinishReason`, `CanonicalStreamEvent`, `ProtocolCapabilities`, `ProviderExtensions`.

**Why:** Rule 2 (don't flatten) and Rule 5 (typed domain models). An untyped `serde_json::Value` internal representation would lose compile-time guarantees about field presence and type correctness. The typed model catches translation errors at compile time rather than runtime.

**Trade-off:** More code to define and maintain the types. But the type safety prevents entire classes of bugs (wrong field names, missing fields, type mismatches) that would be silent with `Value`.

#### Decision: Content Blocks as Enum, Not Flat Union

**What:** `ContentBlock` is a tagged enum with 5 variants: `Text`, `Image`, `ToolUse`, `ToolResult`, `ToolReference`. Messages use `MessageContent` (a `#[serde(untagged)]` enum of `Text(String)` | `Blocks(Vec<ContentBlock>)`).

**Why:** Different content types have different structures. A tool call has `id`, `name`, and `input`; a text block has only `text`. Forcing them into a flat struct would require `Option` fields for every variant's unique fields. The enum makes invalid states unrepresentable.

**Trade-off:** Pattern matching on every content block is more verbose than field access. But the exhaustiveness checking ensures every adapter handles every content type.

#### Decision: Provider Extensions via Typed + JSON Hybrid

**What:** `ProviderExtensions` has named fields (`openai: Option<Value>`, `anthropic: Option<Value>`) plus `ToolDefinition.extra: HashMap<String, Value>` for provider-specific tool metadata.

**Why:** Some extensions are well-known (OpenAI's `strict` mode, Anthropic's `stop_sequence`) and benefit from typed access. Others are arbitrary and need a JSON escape hatch. The hybrid approach provides type safety where it matters and flexibility where it doesn't.

**Known limitation:** Adding a new provider requires modifying the `ProviderExtensions` struct. A `HashMap<String, Value>` keyed by provider name would be more extensible but less type-safe. This is documented as a future improvement.

#### Decision: Protocol-Core as Pure Library (No Axum/Metrics)

**What:** The `protocol-core` crate depends only on `serde`, `serde_json`, `bytes`, `http`, `thiserror`, and `tracing`. It has no dependency on `axum`, `metrics`, or any gateway infrastructure.

**Why:** Separation of concerns. The protocol model should be reusable by any consumer (gateway, SDK, tests, control plane). Tying it to axum would prevent use in non-HTTP contexts (gRPC, CLI tools, tests). The `IntoResponse` implementation for `ProtocolEngineError` lives in the gateway crate, not protocol-core.

**Trade-off:** Error types in protocol-core can't implement `IntoResponse` directly. The gateway must wrap them. This is a small inconvenience for significant decoupling.

#### Decision: SSE Parser at Protocol Framing Level

**What:** `StreamingSseParser` operates on raw bytes and accumulates partial lines across TCP chunks. It does not treat a TCP chunk as an SSE event boundary.

**Why:** Real-world SSE streams split events at arbitrary byte boundaries. A single TCP chunk may contain a partial `data:` line, and a single event may span multiple chunks. The parser must handle:
- `data: {"delta":"hel` (chunk 1) + `lo"}\n\n` (chunk 2)
- `data: first\n\ndata: sec` (chunk 1) + `ond\n\n` (chunk 2)

**Implementation:** The parser maintains a `line_buffer: String` that accumulates incomplete lines. On each `feed()`, it drains complete lines (terminated by `\n`), parses SSE fields, and emits events on blank-line boundaries. The `finish()` method flushes any remaining buffer on connection close.

**Key invariant:** The parser shares field-parsing logic between `feed()` and `finish()` via a `parse_line()` method — eliminating the previous duplication where `finish()` only handled `data:` fields and silently dropped `event:`/`id:`/`retry:`.

#### Decision: Canonical Stream Event Taxonomy

**What:** `CanonicalStreamEvent` has 10 variants covering the union of streaming capabilities from OpenAI and Anthropic:

```text
MessageStart, ContentBlockStart, TextDelta, ToolCallDelta,
ContentBlockStop, MessageDelta, Usage, MessageStop, Error, Ping
```

**Why:** No single protocol has this exact set of events. OpenAI uses `chat.completion.chunk` with a `delta` object; Anthropic uses `content_block_start`/`content_block_delta`/`content_block_stop`. The canonical taxonomy must be a superset that can represent both.

**Trade-off:** Some canonical events have no direct equivalent in one protocol (e.g., Anthropic has no explicit `MessageStart`; OpenAI has no `ContentBlockStart`). The adapter returns `None` for events without a target equivalent, and the caller skips them.

#### Decision: Deferred Tool References as First-Class

**What:** `ToolReference` is a `ContentBlock` variant with `id`, `name`, optional `description`, optional `input_schema`, and `deferred: bool`. It is not silently materialized into a full `ToolDefinition`.

**Why:** Rule 4 (deferred tool semantics). MCP tool discovery exposes lightweight references, not full schemas. A translator that silently loads the full schema would defeat the purpose of deferred loading.

**Current limitation:** The Anthropic adapter degrades `ToolReference` to a text annotation (`"[tool reference: name]"`) because Anthropic's API doesn't support deferred references. This is logged as a warning and documented as a lossy translation.

#### Decision: Capability Matrix with Translation Loss Detection

**What:** `ProtocolCapabilities` declares what each protocol supports (streaming, tools, tool_streaming, multimodal_input, structured_output, reasoning, usage_streaming, deferred_tools). `translation_losses()` computes the set of features lost when translating from one protocol to another.

**Why:** Rule 9 (conformance tests) and Rule 2 (don't flatten). The capability matrix makes incompatibilities explicit rather than silent. When translating from a protocol that supports `structured_output` (OpenAI) to one that doesn't (Anthropic), the system can reject, warn, or approximate — rather than silently dropping the feature.

**Current limitation:** The `translation_losses()` method is defined but not yet called by any adapter during encode/decode. It is purely declarative at this stage. Wiring it into the adapter pipeline is a planned improvement.

#### Decision: Five Loss Policies

**What:** `LossPolicy` enum with 5 variants: `Reject`, `Warn`, `Drop`, `Approximate`, `EncodeAsExtension`.

**Why:** Different features warrant different handling. A `structured_output` format constraint can be approximated via system instructions (`Approximate`). A `reasoning` content block can be encoded as extension metadata (`EncodeAsExtension`). An `audio` content block might need to be `Reject`ed if the target can't handle it at all.

**Current limitation:** Only `Reject` is enforced (maps to HTTP 400). The other policies are defined but not executed — they serve as a framework for future enforcement.

#### Decision: OpenAI Responses Adapter as Stub

**What:** The OpenAI Responses API adapter exists as a module with `capabilities()` returning all `false` and `decode_request()`/`encode_response()` returning `UnsupportedFeature`.

**Why:** The Responses API has a different wire format from Chat Completions (uses `input` instead of `messages`, has `instructions` as top-level, different event types). Full implementation requires the complete Responses API spec. The stub provides the extension point without pretending to work.

**Trade-off:** The capabilities claim `false` for everything, which is honest but means routing to this adapter will fail. This is correct behavior — the system should fail explicitly rather than silently produce wrong output.

#### Decision: Error Type Consolidation

**What:** Two error types exist: `ProtocolEngineError` (thiserror-based, operational) and `LossPolicy` (from `canonical.rs`, used in error variants). The previously separate `ProtocolError`/`ProtocolErrorType` in `canonical.rs` was removed as dead code overlapping with `ProtocolEngineError`.

**Why:** `ProtocolEngineError` is the operational error type used by adapter functions — it has `thiserror` integration, HTTP status code mapping, and metrics category labels. `ProtocolError` was a serializable error struct that duplicated the same categories but was never used by any adapter. Removing it eliminates the DRY violation.

#### Decision: Helper Functions for Repeated Mappings

**What:** Extracted `finish_reason_to_openai()` and `usage_to_chat()` as shared helpers in the OpenAI adapter.

**Why:** The `FinishReason` → OpenAI string mapping appeared identically in `encode_response()` and `encode_stream_event()`. The `Usage` → `ChatUsage` conversion appeared 3 times. Extracting helpers eliminates the duplication and ensures consistent behavior.

#### Decision: Malformed Tool Arguments Logged, Not Rejected

**What:** When OpenAI tool call arguments fail to parse as JSON, the adapter logs a warning and substitutes `{}` (empty object) rather than returning an error.

**Why:** Some models produce malformed tool arguments (e.g., truncated JSON). Rejecting these would cause the entire request to fail. Substituting `{}` allows the request to proceed — the downstream tool will likely return an error, which the model can then correct. The warning ensures the issue is visible in logs.

**Trade-off:** The tool receives empty arguments instead of the malformed ones, which may produce a confusing error message. A future improvement could preserve the raw string and let the tool decide how to handle it.

---

## 6. Protocol Translation Details

### 6.1 OpenAI Chat Completions → Canonical

| OpenAI field | Canonical field | Notes |
|---|---|---|
| `model` | `model` | Direct copy |
| `messages[].role` | `message.role` | `"system"`→`System`, `"developer"`→`System`, `"user"`→`User`, `"assistant"`→`Assistant`, `"tool"`→`Tool` |
| `messages[].content` (string) | `MessageContent::Text` | Simple text |
| `messages[].content` (array) | `MessageContent::Blocks` | Each block decoded by type |
| `messages[].content` (null) | Empty blocks vec | No content |
| `messages[].tool_calls` | `ContentBlock::ToolUse` appended to assistant message | Tool calls become content blocks |
| `messages[].tool_call_id` | `ContentBlock::ToolResult` in tool message | Tool results reference by ID |
| `system` messages | Extracted to `CanonicalRequest.system` | Multiple system messages concatenated with `\n\n` |
| `tools[].function` | `ToolDefinition` | `name`, `description`, `input_schema` (from `parameters`) |
| `tool_choice` | `ToolChoice` | `"auto"`→`Auto`, `"required"`→`Required`, `"none"`→`None`, `{name}`→`Named` |
| `temperature`, `top_p`, `max_tokens` | Direct copy | |
| `stop` | `stop: Vec<String>` | |
| `stream` | `stream: bool` | |
| `response_format` | `ResponseFormat` | `format_type` + optional `json_schema` |
| Unknown fields | `extensions.openai` | `#[serde(flatten)]` catch-all |

### 6.2 Canonical → Anthropic Messages

| Canonical field | Anthropic field | Notes |
|---|---|---|
| `model` | `model` | Direct copy |
| `max_tokens` | `max_tokens` | Required (defaults to 4096 if absent) |
| `system` | `system` (top-level) | String or array of text blocks |
| `messages` | `messages` | Role mapping: `User`→`"user"`, `Assistant`→`"assistant"`, `Tool`→merged into user message |
| `ToolUse` blocks | `tool_use` content blocks | Within assistant messages |
| `ToolResult` blocks | `tool_result` content blocks | Within user messages (Anthropic requirement) |
| `Text` blocks | `text` content blocks | Direct |
| `Image` blocks | `image` content blocks | URL or base64 source |
| `ToolReference` | `[tool reference: name]` text | **Lossy** — logged as warning |
| `tools` | `tools` | `name`, `description`, `input_schema` |
| `tool_choice` `Auto` | `tool_choice` `auto` | |
| `tool_choice` `Required` | `tool_choice` `any` | |
| `tool_choice` `None` | Tools omitted entirely | Anthropic has no "none" option |
| `tool_choice` `Named` | `tool_choice` `tool` | |
| `stop` | `stop_sequences` | |
| `response_format` | **Dropped** | Logged as info; Anthropic has no equivalent |

### 6.3 Anthropic Messages → Canonical

| Anthropic field | Canonical field | Notes |
|---|---|---|
| `model` | `model` | |
| `max_tokens` | `max_tokens` | |
| `system` (string) | `system: Text` | |
| `system` (array) | `system: Blocks` | Each `{"type":"text","text":...}` becomes `SystemBlock` |
| `messages[].content` (string) | `MessageContent::Text` | |
| `messages[].content` (array) | `MessageContent::Blocks` | Each block decoded by type tag |
| `tool_use` blocks | `ContentBlock::ToolUse` | |
| `tool_result` blocks | `ContentBlock::ToolResult` | Content flattened to text if array |
| `tools` | `ToolDefinition` | `input_schema` is required |
| `tool_choice` `auto` | `ToolChoice::Auto` | |
| `tool_choice` `any` | `ToolChoice::Required` | |
| `tool_choice` `tool` | `ToolChoice::Named` | |
| `stop_sequences` | `stop` | |

### 6.4 Canonical → OpenAI Chat Completions

| Canonical field | OpenAI field | Notes |
|---|---|---|
| `id` | `id` | |
| `model` | `model` | |
| `content` (text blocks) | `message.content` | Concatenated with `\n` |
| `content` (tool_use blocks) | `message.tool_calls` | Each becomes `{id, type:"function", function:{name, arguments}}` |
| `finish_reason` `Stop` | `"stop"` | |
| `finish_reason` `Length` | `"length"` | |
| `finish_reason` `ToolCalls` | `"tool_calls"` | |
| `finish_reason` `ContentFilter` | `"content_filter"` | |
| `usage` | `usage` | `input_tokens`→`prompt_tokens`, `output_tokens`→`completion_tokens` |
| `Image` blocks | **Rejected** | HTTP 501 — OpenAI Chat doesn't support image output |

### 6.5 Streaming Translation

**OpenAI SSE → Canonical:**
```
data: {"choices":[{"delta":{"role":"assistant"}}]}
  → CanonicalStreamEvent::MessageStart
data: {"choices":[{"delta":{"content":"Hello"}}]}
  → CanonicalStreamEvent::TextDelta { text: "Hello" }
data: {"choices":[{"delta":{},"finish_reason":"stop"}]}
  → CanonicalStreamEvent::MessageDelta { stop_reason: Stop }
data: [DONE]
  → (end of stream)
```

**Canonical → Anthropic SSE:**
```
MessageStart → event: message_start
ContentBlockStart(text) → event: content_block_start
TextDelta → event: content_block_delta (type: text_delta)
ContentBlockStop → event: content_block_stop
MessageDelta → event: message_delta
MessageStop → event: message_stop
```

**Canonical → OpenAI SSE:**
```
MessageStart → first chunk with role:"assistant"
TextDelta → chunk with delta.content
ToolCallDelta → chunk with delta.tool_calls[].function.arguments
MessageDelta → chunk with finish_reason
```

---

## 7. Testing Strategy

### 7.1 Test Pyramid

```
           E2E / production-like
                  /\
                 /  \
            integration
               /    \
              /      \
           unit/property
```

### 7.2 Protocol-Core Tests (71 total)

| Category | Count | What's tested |
|----------|-------|---------------|
| Unit tests (in-crate) | 31 | SSE parser (15), OpenAI adapter (7), Anthropic adapter (5), OpenAI Responses stub (2), capability declaration (2) |
| Streaming boundary tests | 18 | Byte-level splitting, split in middle of data/event-type, large events (100KB), many events in one chunk, connection termination flush, CRLF, comments, unknown fields, format-then-parse roundtrip, realistic OpenAI/Anthropic stream simulations |
| Translation e2e tests | 22 | OpenAI→Anthropic (simple, tool calls, multimodal), Anthropic→OpenAI (simple, tool use response), round-trips (OpenAI response, Anthropic request), streaming translation (text delta, tool call delta), capability matrix, translation loss detection, SSE parse integration, error handling (invalid JSON, missing fields, image output rejection), edge cases (empty content, multiple system messages, system as array, usage preservation) |

### 7.3 Phase 1 Tests (38 total)

| Category | Count |
|----------|-------|
| Unit tests | 13 |
| Integration tests | 15 |
| Cancellation tests | 2 |
| Load tests | 4 |
| Mock upstream tests | 4 |

### 7.4 Golden Fixture Pattern

Every translation test follows:
```text
source JSON → decode → canonical → encode → expected target JSON
```

Round-trip tests verify:
```text
OpenAI → Canonical → OpenAI  (semantic equivalence)
Anthropic → Canonical → Anthropic  (semantic equivalence)
```

### 7.5 Test Infrastructure

- `crates/test-harness`: In-process spawn helpers (`spawn_gateway`, `spawn_json_stack`, `spawn_sse_stack`, `dead_upstream_addr`, `post_hyper`, `get_hyper`)
- `crates/mock-upstream`: Configurable mock with `MockConfig` (mode, chunks, chunk_size, ttfb, chunk_delay, json_body) and `MockState` counters
- `apps/gateway/benches/harness.rs`: Benchmark-specific spawn helpers

---

## 8. Performance

### 8.1 Phase 1 Benchmarks

Measured with Criterion on Rust 1.88, 200 samples (non-streaming) / 50 samples (SSE streaming):

```text
simple_proxy/direct       mean ≈ 43.5 µs  (client → mock upstream)
simple_proxy/via_gateway  mean ≈ 148.7 µs (client → gateway → mock upstream)
Gateway overhead:         ≈ 105 µs  (0.105 ms)

streaming_proxy/direct_sse       mean ≈ 45.1 µs
streaming_proxy/via_gateway_sse  mean ≈ 66.6 µs
SSE streaming overhead:          ≈ 21.5 µs (0.0215 ms)
```

### 8.2 Performance Gates

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Simple proxy p50 | < 1 ms | ≈ 0.105 ms | ✅ |
| Simple proxy p95 | < 2 ms | well under | ✅ |
| Simple proxy p99 | < 5 ms | well under | ✅ |
| Streaming gateway-added delay | low-ms range | ≈ 0.022 ms | ✅ |

### 8.3 Hot-Path Rules

Avoid on the request hot path:
- DB lookups, remote registry lookups
- Synchronous logging to external systems
- Repeated config parsing or route compilation
- Connection establishment when pooling is possible
- Unnecessary JSON transformations
- Copying large buffers

Prefer:
- Immutable config snapshots
- Pre-parsed config
- Pooled connections
- Streaming
- Bytes-oriented processing
- Bounded queues
- Lock-free/read-optimized structures

---

## 9. Code Quality Audit

A three-dimensional review was performed after Phase 2 implementation:

### 9.1 Clean Code Review (7 must-fix, 8 warnings, 8 smells)

**Must-fix items (all resolved):**
1. Duplicate `LossPolicy`/`LossyPolicy` enums → consolidated to single `LossPolicy`
2. Identical if/else branches in Anthropic adapter → removed dead conditional
3. Duplicated `finish_reason` mapping → extracted `finish_reason_to_openai()` helper
4. `decode_request` 167 lines → documented as tech debt (function splitting)
5. `encode_request` 168 lines → documented as tech debt
6. `encode_stream_event` 163/101 lines → documented as tech debt
7. `finish()` parser dropped non-data fields → fixed to parse all field types

**Warnings resolved:**
- `encode_content_blocks` returned `Result` but never failed → changed to infallible return
- Malformed tool args silently replaced → now logged at warn level
- OpenAI Responses capabilities claimed features the stub can't deliver → set to `false`
- Unused `ProtocolError`/`ProtocolErrorType` → removed
- Duplicated usage conversion ×3 → extracted `usage_to_chat()` helper

**Remaining tech debt (documented):**
- Function length (M4-M7): `decode_request`, `encode_request`, `encode_stream_event` need splitting
- `ProviderExtensions` not extensible to new providers
- `ToolDefinition.extra` is untyped catch-all
- `.clone().into_blocks()` in adapter loops (performance)
- Redundant doc comments
- Two overlapping error type systems partially consolidated

### 9.2 Protocol Fidelity Review (3 compliance gaps, 7 recommendations)

**Compliance gaps:**
1. OpenAI `input_audio` silently dropped → needs `AudioContent` variant in canonical model
2. Anthropic `thinking` blocks silently dropped → needs reasoning content block in canonical model
3. `translation_losses()` method exists but is never invoked by any adapter

**Recommendations:**
- Wire `translation_losses()` into adapter encode/decode paths
- Add `AudioContent` and reasoning/thinking variant to canonical model
- Fix `ToolReference` encoding in Anthropic adapter
- Add input-size validation at decode boundary
- Add `AudioDelta` variant to `CanonicalStreamEvent`
- Wire tool streaming buffering fallback
- Handle structured `ToolResult` content without flattening to text

### 9.3 SSE Parser Review (1 correctness fix, 6 coverage gaps)

**Correctness fix applied:**
- `finish()` now parses `event:`/`id:`/`retry:` from remaining buffer (previously only parsed `data:`)

**Remaining coverage gaps (documented):**
- Empty `data:` value test
- Non-numeric `retry:` value test
- `finish()` with non-data lines in buffer test
- Bare `\r` behavior documentation
- Multiple consecutive blank lines test
- `\r\n` split across chunks test

---

## 10. Security Model

### 10.1 Threat Model

The gateway handles:
- LLM API credentials (in transit, never logged)
- User prompts and potentially sensitive data
- Tool credentials
- MCP server connections
- Network routing (egress paths)
- Arbitrary upstream endpoints

### 10.2 Security Properties

| Property | Implementation |
|----------|---------------|
| No secrets in logs | Error responses do not include auth headers; prompt content not logged by default |
| Hop-by-hop stripping | `Connection`, `Host`, `TE`, `Transfer-Encoding`, `Upgrade`, `Proxy-*` stripped |
| Host header rewriting | Derived from lane URL, not forwarded from client |
| Typed errors | Internal details never leaked to clients |
| No DB on hot path | Config is memory-resident; no credential lookups during request handling |
| Protocol translation | Cannot accidentally expose credentials (extensions are body-level, not header-level) |

### 10.3 Known Gaps (Phase 2)

- No authentication middleware (Phase 3+)
- No input-size validation at protocol decode boundary
- No tenant isolation (Phase 7+)
- No SSRF protections beyond URL parsing (Phase 7+)

---

## 11. File Inventory

### 11.1 Rust Source Files (28 files, 7,486 lines)

| File | Lines | Purpose |
|------|-------|---------|
| `apps/gateway/src/config/mod.rs` | 495 | TOML config → immutable ConfigSnapshot |
| `apps/gateway/src/proxy/mod.rs` | 308 | Proxy handler (hot path) |
| `apps/gateway/src/errors/mod.rs` | 256 | Typed error hierarchy |
| `apps/gateway/src/observability/mod.rs` | 174 | Tracing + Prometheus metrics |
| `apps/gateway/src/server/mod.rs` | 162 | GatewayServer binding |
| `apps/gateway/src/transport/mod.rs` | 158 | Header filtering |
| `apps/gateway/src/upstream/mod.rs` | 39 | hyper client builder |
| `apps/gateway/src/lib.rs` | 19 | Library root |
| `apps/gateway/src/main.rs` | 35 | Binary entry point |
| `crates/protocol-core/src/adapters/anthropic_messages/mod.rs` | 1,030 | Anthropic Messages adapter |
| `crates/protocol-core/src/adapters/openai_chat/mod.rs` | 1,016 | OpenAI Chat Completions adapter |
| `crates/protocol-core/src/canonical.rs` | 635 | Typed canonical model |
| `crates/protocol-core/src/sse.rs` | 345 | Streaming SSE parser |
| `crates/protocol-core/src/error.rs` | 130 | ProtocolEngineError |
| `crates/protocol-core/src/adapters/openai_responses/mod.rs` | 77 | OpenAI Responses stub |
| `crates/protocol-core/src/adapters/mod.rs` | 9 | Adapter module root |
| `crates/protocol-core/src/lib.rs` | 20 | Library root |
| `crates/mock-upstream/src/sse.rs` | 192 | SSE event generator |
| `crates/mock-upstream/src/lib.rs` | 179 | MockUpstream spawn/shutdown |
| `crates/mock-upstream/src/app.rs` | 135 | Mock Axum router |
| `crates/mock-upstream/src/bin/mock-upstream.rs` | 82 | Mock CLI binary |
| `crates/test-harness/src/lib.rs` | 209 | Test spawn helpers |
| `apps/gateway/tests/proxy_integration.rs` | 369 | Integration tests |
| `crates/protocol-core/tests/translation_e2e.rs` | 623 | Translation e2e tests |
| `crates/protocol-core/tests/streaming_boundary.rs` | 280 | SSE boundary tests |
| `apps/gateway/tests/cancellation_integration.rs` | 105 | Cancellation tests |
| `apps/gateway/tests/load_test.rs` | 92 | Load tests |
| `apps/gateway/benches/proxy_latency.rs` | 221 | Criterion benchmarks |
| `apps/gateway/benches/harness.rs` | 91 | Benchmark helpers |

### 11.2 Documentation Files (15 files, 1,751 lines)

| File | Lines | Purpose |
|------|-------|---------|
| `docs/architecture.md` | 403 | System topology, domain model, data-plane lifecycle |
| `docs/testing.md` | 185 | Test strategy, pyramid, fixtures, fault injection |
| `docs/performance.md` | 160 | Performance contract, benchmarks, hot-path rules |
| `docs/development.md` | 133 | Local dev setup, CI, coding standards |
| `docs/state.md` | 123 | Current implementation state |
| `docs/security.md` | 123 | Threat model, security zones, secrets |
| `docs/protocols.md` | 131 | Protocol translation contract |
| `docs/workflow-ir.md` | 101 | Workflow IR design |
| `docs/roadmap.md` | 99 | Phase breakdown (0-8) |
| `docs/observability.md` | 90 | Metrics, tracing, debugging |
| `docs/mcp-skills.md` | 121 | MCP and Skills design |
| `docs/adr-0001-stack.md` | 24 | Core stack decision |
| `docs/adr-0002-lanes.md` | 26 | Lane routing decision |
| `docs/adr-0003-protocol-fidelity.md` | 16 | Protocol semantics decision |
| `docs/adr-0004-control-data-plane.md` | 16 | Control/data plane separation |

### 11.3 Configuration & Infrastructure

| File | Purpose |
|------|---------|
| `Cargo.toml` | Workspace root |
| `Cargo.lock` | Dependency lockfile |
| `rust-toolchain.toml` | Rust stable channel |
| `rustfmt.toml` | Formatting config (max_width=100, edition=2024) |
| `.github/workflows/ci.yml` | CI pipeline |
| `.githooks/pre-commit` | Pre-commit rust-policy check |
| `.claude/settings.json` | Claude Code hooks + permissions |
| `.claude/hooks/check-rust-policy.sh` | Rust policy enforcement |
| `.claude/skills/architecture-guard/SKILL.md` | Architecture rule checker |
| `.claude/skills/scaffold-phase/SKILL.md` | Phase scaffolding |
| `.claude/agents/protocol-fidelity-reviewer.md` | Protocol review agent |
| `.claude/agents/hot-path-auditor.md` | Performance review agent |

---

## 12. Dependency Summary

### 12.1 Runtime Dependencies

| Crate | Version | Used by | Purpose |
|-------|---------|---------|---------|
| tokio | 1.53 | gateway, protocol-core (dev) | Async runtime |
| hyper | 1.11 | gateway | HTTP server + client |
| hyper-util | 0.1 | gateway | Connection pooling |
| axum | 0.8 | gateway | Application routing |
| tower | 0.5 | gateway | Middleware (timeout, limit, load-shed) |
| tower-http | 0.7 | gateway | HTTP middleware (trace, request-id) |
| http | 1 | gateway, protocol-core | HTTP types |
| http-body / http-body-util | 1 / 0.1 | gateway | Body streaming |
| bytes | 1 | gateway, protocol-core | Byte buffers |
| serde / serde_json | 1 / 1 | all | Serialization |
| toml | 1.1 | gateway | Config parsing |
| tracing / tracing-subscriber | 0.1 / 0.3 | gateway, protocol-core | Structured logging |
| metrics / metrics-exporter-prometheus | 0.24 / 0.18 | gateway | Prometheus metrics |
| thiserror | 2 | gateway, protocol-core | Error derive |
| clap | 4.6 | gateway | CLI argument parsing |
| uuid | 1.26 | gateway | Request IDs |
| url | 2 | gateway | URL parsing |
| anyhow | 1 | gateway | Error context |
| futures-util | 0.3 | gateway, mock-upstream | Stream utilities |

### 12.2 Dev Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| criterion | 0.8 | Benchmarking |
| proptest | 1 | Property-based testing |

---

## 13. What's Next (Remaining Phase 2 + Future Phases)

### Remaining Phase 2

- [ ] OpenAI Responses adapter (full implementation)
- [ ] Performance benchmarks for translation overhead
- [ ] Wire `translation_losses()` into adapter encode/decode paths
- [ ] Add `AudioContent` and reasoning/thinking variant to canonical model
- [ ] Fix `ToolReference` encoding in Anthropic adapter
- [ ] Add missing SSE boundary tests
- [ ] Split oversized adapter functions (decode_request, encode_request, encode_stream_event)
- [ ] Documentation updates (PROTOCOLS.md, PERFORMANCE.md, TESTING.md)

### Future Phases

- **Phase 3:** Lanes and routing (lane registry, endpoint registry, health checks, connection pools per lane, WireGuard integration, fallback routing)
- **Phase 4:** Workflow compiler (schema, validator, compiler, execution IR, fast-path classification, runtime executor)
- **Phase 5:** Visual editor (React Flow canvas, node library, publish/version workflow)
- **Phase 6:** MCP and Skills (registry, metadata index, dynamic discovery, deferred tool activation)
- **Phase 7:** Production security (secret manager, tenant isolation, policy engine, SSRF protections, sandboxed tool workers, audit log)
- **Phase 8:** Advanced routing (latency/cost/capacity-aware routing, health scoring, adaptive capability retrieval)

---

*Report generated 2026-09-10. This document is the source of truth for Phase 1 and Phase 2 implementation state.*
