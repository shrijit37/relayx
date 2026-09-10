# ADR-0001: Core Stack

## Status
Accepted

## Decision

Use:

- Rust/Tokio for the performance-critical data plane
- TypeScript/Fastify for the initial control plane
- React/React Flow for the workflow editor
- PostgreSQL for durable configuration/state

## Rationale

The gateway is network and streaming heavy and benefits from Rust's predictable resource usage and async runtime. TypeScript is productive for CRUD/control-plane APIs. React Flow directly matches the graph editor requirement.

## Consequences

- two primary backend languages
- shared domain schemas need disciplined ownership
- more deployment artifacts
- clear separation between control and data plane
