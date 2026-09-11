# TESTING.md — Test Strategy

## Test pyramid

```text
             E2E / production-like
                    /\
                   /  \
              integration
                 /    \
                /      \
             unit/property
```

## Phase 1 test coverage

### Unit tests (17)

- Config parsing, validation, compilation, route matching (5)
- Error status codes, JSON error body serialization (2)
- Transport: hop-by-hop header filtering, Host header building (3)
- Upstream client builder smoke test (1)
- Server healthz/ready endpoints (2)
- Proxy: upstream request rewriting (1)
- Protocol engine: creation, same-pair rejection, capabilities, loss detection (4)

### Integration tests (15)

- JSON round-trip (client → gateway → mock → gateway → client)
- Query string preservation
- 404 on unknown path and unmatched method
- Large request body (1MB+)
- SSE streaming passthrough (all chunks arrive)
- Upstream 4xx passthrough
- Upstream 5xx passthrough
- Upstream connection refused → 502
- Upstream timeout → 504
- Slow upstream with chunk delays
- Timeout during streaming (high TTFB → 504)
- Concurrent upstream disconnect (20 parallel requests)
- Healthz/ready endpoints
- Metrics exposure (Prometheus format)

### Cancellation tests (2)

- Client disconnect mid-stream → upstream task dropped
- Slow client receives backpressure (no unbounded buffering)

### Load tests (4)

- 1 concurrent request
- 10 concurrent requests
- 100 concurrent requests
- Connection reuse (50 sequential requests → fewer connections)

### Mock upstream tests (4)

- SSE event wire format
- SSE event builder preserves shape
- SSE response builds with correct status/content-type
- SSE error-at-zero returns configured status

### Benchmarks (4 groups)

- `simple_proxy/direct` — client → mock (baseline)
- `simple_proxy/via_gateway` — client → gateway → mock
- `streaming_proxy/direct_sse` — SSE baseline
- `streaming_proxy/via_gateway_sse` — SSE through gateway

## Running tests

```bash
# All tests
cargo test --all-features --workspace

# Unit tests only
cargo test --all-features -p relay-gateway --lib

# Integration tests only
cargo test --all-features -p relay-gateway --test proxy_integration
cargo test --all-features -p relay-gateway --test cancellation_integration
cargo test --all-features -p relay-gateway --test load_test

# Benchmarks
cargo bench --bench proxy_latency -p relay-gateway
```

## Test harness

The `crates/test-harness` crate provides in-process spawn helpers:
- `spawn_gateway(addr)` — gateway with two routes to a mock upstream
- `spawn_gateway_with_timeout(addr, ms)` — gateway with custom request timeout
- `spawn_json_stack(body)` — mock (JSON mode) + gateway
- `spawn_sse_stack(chunks, chunk_size)` — mock (SSE mode) + gateway
- `dead_upstream_addr()` — port that will refuse connections
- `post_hyper(url, body, headers)` / `get_hyper(url)` — raw HTTP client helpers

## Protocol conformance (Phase 2, COMPLETE)

131 tests in `crates/protocol-core/`:

- 38 unit tests across SSE parser + 3 adapters
- 26 streaming boundary tests (byte-level splitting, format roundtrip, edge cases)
- 41 translation e2e tests (OpenAI↔Anthropic, Responses, tool calls, streaming, reasoning, capability loss detection)
- 13 canonical/error tests (serde roundtrips, error variants, capability helpers)
- 5 property tests (SSE parser fuzzing, canonical/usage serde roundtrip)
- Benchmark suite in `benches/translation_throughput.rs` (4 groups: request_decode, response_encode, stream_event_encode, cross_adapter)

Test:

- ordinary text
- multimodal blocks
- multiple messages
- tool calls
- tool results
- streaming
- structured output
- errors
- cancellation
- provider-specific fields
- deferred tool references

## Golden fixtures (Phase 2+)

Store request/response fixtures with normalized timestamps and IDs.

A translation test should fail when:

- required semantics disappear
- event ordering changes unexpectedly
- tool IDs are changed incorrectly
- provider extension data is silently discarded

## Workflow compiler tests (Phase 4+)

Test:

- valid graph compilation
- invalid graph rejection
- deterministic output
- capability mismatch
- policy rejection
- fast-path detection
- fallback compilation

## Network lane tests (Phase 3+)

Use isolated test routes/namespaces where possible.

Verify:

- request exits through intended lane
- unhealthy lane is rejected
- fallback chooses permitted alternate lane
- connection pools do not cross lane boundaries

## Security tests (Phase 7+)

Include:

- SSRF attempts
- secret leakage tests
- tenant escape attempts
- unauthorized tool execution
- malicious MCP metadata
- prompt/tool injection scenarios

## Load tests

Use a controllable mock upstream (`crates/mock-upstream`).

Required outputs:

- throughput
- p50/p95/p99 latency
- CPU
- memory
- allocations
- connection reuse
- queue depth
- errors

## Fault injection

The mock upstream supports:

- configurable TTFB latency
- configurable chunk delay
- error at specific chunk index (`x-mock-error-at` header)
- custom error status (`x-mock-error-status` header)
- connection resets

The system should fail deterministically and observably.
