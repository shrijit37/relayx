# ARCHITECTURE.md
> **Status:** living · **Verified:** 2026-09-16 · **Purpose:** Target system architecture; labels what is implemented vs. planned.

> **Note (Phase 6.5):** This document describes the **target architecture**. Not all connections shown here exist in the current implementation. What is real today: the backend data plane, the control plane (workflows/versions/providers/lanes + publish + run passthrough), and a backend-authoritative frontend — the editor loads/saves/validates/publishes/runs against the control plane and shows no fabricated data. Not yet wired (honest "not available" in the UI): MCP/skill registries, policy enforcement, observability backend, run-history. See [state.md](state.md) and [PHASE6.5_IMPLEMENTATION_REPORT.md](archive/PHASE6.5_IMPLEMENTATION_REPORT.md) for what is actually wired today.

## 1. Product definition

The system is a **visual AI gateway and agent infrastructure orchestrator**.

It is conceptually:

```text
n8n-like workflow editor
        +
LLM gateway
        +
protocol translation
        +
provider routing
        +
network/VPN lanes
        +
MCP/tool discovery
        +
Skill discovery
        +
policy/observability
```

The goal is not merely to proxy HTTP. The goal is to make model execution, capabilities, network egress, and fallback behavior declarative and composable.

## 2. System topology

```text
                         +----------------------+
                         |      React Flow      |
                         |   visual workflow    |
                         +----------+-----------+
                                    |
                                    v
                         +----------------------+
                         |     Control Plane     |
                         |----------------------|
                         | workflows             |
                         | providers             |
                         | lanes                 |
                         | MCP registry          |
                         | skill registry        |
                         | policies              |
                         | compiler              |
                         +----------+-----------+
                                    |
                          immutable snapshots
                                    |
                                    v
+---------+                 +----------------------+
| Client  |  HTTP/SSE/WS -> |    Rust Data Plane   |
+---------+                 |----------------------|
                          ->| auth/policy          |
                          ->| route resolver       |
                          ->| lane selector        |
                          ->| protocol adapter     |
                          ->| stream proxy         |
                          ->| capability runtime   |
                           +----------+-----------+
                                      |
                 +--------------------+--------------------+
                 |                    |                    |
                 v                    v                    v
          +-------------+      +-------------+      +-------------+
          | Anthropic   |      | OpenAI      |      | Other LLMs  |
          +-------------+      +-------------+      +-------------+
                 |                    |                    |
             direct/VPN          direct/VPN            direct/VPN
                 |                    |                    |
                 +--------------------+--------------------+
                                      |
                                network lanes
```

## 3. Core domain model

### Provider

A logical model API provider.

```text
Provider
- id
- name
- protocol_family
- credentials_ref
- capabilities
- endpoints[]
```

### Endpoint

A concrete upstream URL/API target.

```text
Endpoint
- id
- provider_id
- base_url
- region
- protocol
- health
- limits
```

### Lane

The key routing abstraction:

```text
Lane = Provider/Endpoint + Network Route + Policy + Connection Pool
```

Example:

```text
anthropic-us-vpn
  endpoint = Anthropic production
  network = wireguard-us-01
  policy = production-standard
  pool = warm
```

### Capability

Unified catalog concept covering tools, MCP servers, Skills, and other agent extensions.

```text
Capability
- id
- type: tool | mcp | skill
- metadata
- permissions
- discovery_index
- source
- version
```

### Workflow

The user-facing graph.

### Execution Plan

A validated, optimized, runtime-oriented representation compiled from the workflow graph.

## 4. Plan compiler

```text
React Flow graph
      |
      v
schema validation
      |
      v
semantic validation
      |
      v
capability resolution
      |
      v
policy compilation
      |
      v
route/lane validation
      |
      v
optimization
      |
      v
Execution Plan vN
```

Optimizations can include:

- constant folding
- removing unreachable nodes
- pre-resolving static routes
- precomputing capability sets
- grouping sequential transforms
- selecting adapter pipelines
- preloading lane metadata

The compiler must be deterministic for the same input and dependency versions.

## 5. Data-plane request lifecycle

```text
HTTP request
   |
   v
authenticate
   |
   v
resolve tenant/project/workflow snapshot
   |
   v
select execution plan
   |
   +--> simple fast path?
   |       |
   |      yes -> direct route/lane -> provider
   |
   no
   |
   v
execute plan
   |
   +--> capability discovery/cache
   +--> provider selection
   +--> lane selection
   +--> translation
   +--> tool/MCP execution if allowed
   +--> fallback/retry policy
   |
   v
stream result
```

## 6. Fast path

The system should detect plans that reduce to:

```text
input -> route -> lane -> provider -> stream
```

and use a minimal execution path.

No workflow interpreter loop should be required for this case.

## 7. Dynamic MCP discovery

There are two layers:

### Registry/discovery layer

Stores lightweight metadata and searchable descriptions.

### Activation layer

Loads the full schema/tool definition only when selected.

```text
MCP registry
   -> retrieve candidates
   -> policy filter
   -> load selected tool metadata/schema
   -> expose to agent/provider
```

The gateway must preserve protocol semantics such as deferred tool loading/tool references where the upstream client/provider uses them.

## 8. Skill discovery

Skills are loaded progressively:

```text
Skill metadata
   -> instructions
   -> references/scripts/resources
```

Skill metadata should be cheap to index. Large references should not become unconditional request context.

## 9. Protocol architecture

Use a canonical internal event model but retain protocol-specific extensions.

```text
Client protocol
      |
      v
Ingress adapter
      |
      v
Canonical request/events
      |
      v
Execution/runtime
      |
      v
Provider adapter
      |
      v
Provider protocol
```

The canonical model is not allowed to erase information that can be preserved through an extension field.

## 10. Streaming

Streaming is event-based.

```text
upstream event
  -> provider adapter
  -> canonical event
  -> policy/telemetry
  -> client adapter
  -> immediate flush
```

Do not buffer full responses by default.

Backpressure must propagate through the pipeline.

## 11. Network lanes

The application should select a preconfigured egress lane rather than implement VPN protocols itself.

```text
Lane resolver
   -> socket/connection policy
   -> network namespace/interface/proxy
   -> provider endpoint
```

Possible implementations:

- WireGuard interface
- network namespace
- dedicated proxy
- SOCKS/HTTP CONNECT
- cloud egress gateway

The lane should expose health and latency signals.

## 12. Routing

Routing inputs can include:

- requested model
- provider preference
- geography
- compliance policy
- lane health
- latency
- token cost
- capacity
- tenant policy
- task type
- capability requirements

Routing is deterministic when the policy produces a unique choice; otherwise weighted/health-aware strategies may apply.

## 13. Failure model

Failures are classified:

```text
client_error
provider_error
translation_error
network_error
lane_unhealthy
rate_limited
capability_missing
auth_error
policy_denied
timeout
internal_error
```

Only retry failures that are safe and idempotent according to the request and provider semantics.

## 14. Observability

Required dimensions:

- request_id
- tenant_id
- workflow_version
- route_id
- lane_id
- provider
- endpoint
- protocol_in
- protocol_out
- tool/skill identifiers
- retry_count
- first_byte_ms
- total_ms
- upstream_ms
- gateway_ms
- cache hits/misses
- error class

Secrets and prompt content should not be logged by default.

## 15. Scaling model

Horizontal scaling is straightforward for stateless gateway instances if:

- configuration snapshots are distributable
- connection pools are local and warm
- durable state is outside the process
- network lane topology is deterministic

Control plane and data plane can scale independently.
