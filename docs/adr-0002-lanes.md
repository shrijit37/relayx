# ADR-0002: Lane as the Primary Routing Abstraction

## Status
Accepted

## Decision

Model a route as a **lane** that binds:

```text
provider/endpoint
+
network egress
+
policy
+
connection pool
```

## Rationale

The product needs routing decisions that cannot be expressed as model->provider only. The network path is a first-class operational constraint.

## Consequences

Routing, health, connection pooling, and network policy must all understand lane identity. Pools cannot be shared across incompatible lanes.
