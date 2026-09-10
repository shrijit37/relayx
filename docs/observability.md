# OBSERVABILITY.md

## Principles

Observability must explain where latency and failures originate without requiring sensitive payload logging.

## Trace model

```text
request
  ├── request ID (UUID v4, per-request)
  ├── route selection
  ├── lane selection
  ├── upstream connect/acquire
  ├── TTFB (time to first byte)
  ├── streaming (per-frame, with frame timeout)
  └── response completion
```

## Implemented metrics (Phase 1)

All metrics use the `relayx_` prefix and are exposed at `/metrics` (Prometheus format).

### Counters

| Metric | Labels | Description |
|--------|--------|-------------|
| `relayx_request_total` | `status`, `lane` | Total requests served |
| `relayx_bytes_in` | — | Total bytes received from clients |
| `relayx_bytes_out` | — | Total bytes sent to clients |
| `relayx_errors_total` | `category` | Errors by category (client_error, provider_rejection, network_error, timeout, cancelled, internal) |
| `relayx_timeout_total` | `lane` | Requests terminated by per-request timeout |
| `relayx_route_selected_total` | `route` | Route match counter |
| `relayx_lane_selected_total` | `lane` | Lane selection counter |

### Histograms

| Metric | Labels | Description |
|--------|--------|-------------|
| `relayx_request_duration_ms` | `status`, `lane` | End-to-end request duration |
| `relayx_upstream_connect_ms` | `lane` | Time to establish upstream connection |
| `relayx_upstream_ttfb_ms` | `lane` | Time from request start to upstream response headers |
| `relayx_upstream_body_duration_ms` | `lane` | Duration of upstream body streaming |

### Gauges (via RAII guards)

| Metric | Labels | Description |
|--------|--------|-------------|
| `relayx_active_requests` | `lane` | Currently in-flight requests (incremented on entry, decremented on drop) |

### Admin endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /healthz` | Liveness probe — returns `{"status":"ok"}` |
| `GET /ready` | Readiness probe — returns `{"status":"ready"}` |
| `GET /metrics` | Prometheus text format |

## Logging rules

Default logs should include identifiers and metadata, not full prompt/completion contents.

Sensitive content logging is opt-in and must be access controlled.

Structured JSON logging via `tracing-subscriber` with env-filter (`RUST_LOG`).

## Debugging a slow request

The first diagnostic question is:

```text
Was the delay introduced by the gateway or upstream?
```

Use:

```text
relayx_upstream_ttfb_ms  (gateway-added overhead before first byte)
relayx_upstream_connect_ms  (TCP connection establishment time)
```

as a first approximation, then inspect internal spans (request ID is in every span).

## Future metrics (Phase 2+)

- `gateway_overhead_ms` (derived: request_duration - upstream_ttfb)
- `translation_success_total` / `translation_failure_total`
- `tool_registry_lookup_total` / `tool_registry_cache_hit_total`
- `active_streams`, `connection_pool_size`, `connection_pool_reuse_ratio`
- per-provider latency breakdown
