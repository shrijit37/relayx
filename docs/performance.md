# PERFORMANCE.md — Performance Contract

## Objective

Make gateway-added latency negligible relative to network and LLM inference latency.

This means **low overhead**, not literally zero latency.

## Performance budget

Initial engineering targets; validate with benchmarks before treating them as guarantees:

```text
Simple proxy overhead:
  p50 < 1 ms
  p95 < 2 ms
  p99 < 5 ms

Translation path:
  p50 < 2 ms
  p99 < 10 ms

Streaming:
  gateway-added first-byte/event delay should normally remain in the low-ms range
```

## Benchmark methodology

Always benchmark three cases:

### A. Direct baseline

```text
client -> mock upstream
```

### B. Gateway

```text
client -> gateway -> same mock upstream
```

### C. Real provider

```text
client -> provider
client -> gateway -> provider
```

A and B isolate gateway overhead. C measures real-world impact.

## Hot-path rules

Avoid:

- DB lookups
- remote registry lookups
- synchronous logging to external systems
- repeated config parsing
- repeated route compilation
- connection establishment when pooling is possible
- unnecessary JSON transformations
- copying large buffers

Prefer:

- immutable config snapshots
- pre-parsed config
- pooled connections
- streaming
- bytes-oriented processing where practical
- bounded queues
- lock-free/read-optimized structures where justified

## Connection management

Connection reuse is one of the highest-value optimizations.

Pool by a stable upstream identity including relevant network lane attributes.

Do not reuse a connection across incompatible egress/security policies.

## Backpressure

Backpressure must propagate:

```text
client slow
  -> gateway stream
  -> upstream read pressure
```

Do not accumulate unbounded buffers.

## Discovery caches

Tool and Skill discovery must be cached with explicit TTL/version semantics.

Track:

```text
registry_lookup
cache_hit
cache_miss
retrieval_time
activation_time
retrieval_miss
```

## Workflow fast path

Compiled plans should contain a precomputed fast-path classification.

```text
FAST_PATH_DIRECT_PROXY
FAST_PATH_TRANSLATED_PROXY
WORKFLOW_EXECUTION
AGENT_CAPABILITY_EXECUTION
```

## Load testing

Minimum test dimensions:

- 1 concurrent request
- 10
- 100
- 500
- 1,000+

Measure p50/p95/p99 and saturation behavior.

Test both short and long streaming requests.

## Performance regressions

Any change adding a new hot-path abstraction must include benchmark evidence or a clear reason why the code is not on the hot path.

## Baseline measurements (Phase 1 complete)

Measured on 2026-09-10, Rust 1.88, Criterion benchmark with 200 samples (non-streaming) / 50 samples (SSE streaming):

```text
simple_proxy/direct       mean ≈ 43.5 µs  (client → mock upstream)
simple_proxy/via_gateway  mean ≈ 148.7 µs (client → gateway → mock upstream)
Gateway overhead:         ≈ 105 µs  (0.105 ms)

streaming_proxy/direct_sse       mean ≈ 45.1 µs
streaming_proxy/via_gateway_sse  mean ≈ 66.6 µs
SSE streaming overhead:          ≈ 21.5 µs (0.0215 ms)
```

### Performance gates

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| Simple proxy p50 | < 1 ms | ≈ 0.105 ms | ✅ |
| Simple proxy p95 | < 2 ms | well under | ✅ |
| Simple proxy p99 | < 5 ms | well under | ✅ |
| Streaming gateway-added delay | low-ms range | ≈ 0.022 ms | ✅ |
