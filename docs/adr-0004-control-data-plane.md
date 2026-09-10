# ADR-0004: Separate Control and Data Planes

## Status
Accepted

## Decision

Configuration, workflow authoring, registries, secrets references, and compilation belong to the control plane. Serving traffic belongs to the data plane.

## Rationale

This avoids database and control-plane dependencies in the latency-critical request path and permits independent scaling.

## Consequences

Runtime config must be versioned and distributed as immutable snapshots.
