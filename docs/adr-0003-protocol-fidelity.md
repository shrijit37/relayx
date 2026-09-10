# ADR-0003: Preserve Protocol Semantics

## Status
Accepted

## Decision

Use a canonical internal model with explicit extension fields instead of a lowest-common-denominator translation model.

## Rationale

Modern agent clients use provider-specific semantics such as deferred tool references, streaming event types, structured outputs, reasoning metadata, and cache hints. Flattening them causes silent feature loss.

## Consequences

Adapters are more complex. Capability negotiation and conformance testing become mandatory.
