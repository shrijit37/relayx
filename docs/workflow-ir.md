# WORKFLOW_IR.md — Workflow Model and Execution IR
> **Status:** living · **Verified:** 2026-09-16 · **Purpose:** Workflow model and execution IR design.

## Purpose

React Flow is the editor format. The runtime format is a versioned intermediate representation (IR).

## Separation

```text
React Flow UI state
        |
        v
Workflow JSON
        |
        v
Validated semantic graph
        |
        v
Execution IR
```

## Node categories

Initial categories:

- input
- output
- provider
- route
- lane
- transform
- condition
- fallback
- retry
- custom (extension-registered, out-of-process worker RPC)
- MCP discovery
- tool activation
- Skill activation
- agent/model step
- observability

## Example workflow

```json
{
  "version": 1,
  "nodes": [
    {"id": "in", "type": "input"},
    {"id": "route", "type": "route", "config": {"model": "claude-*"}},
    {"id": "lane", "type": "lane", "config": {"lane": "anthropic-us-vpn"}},
    {"id": "mcp", "type": "mcp-discovery", "config": {"query": "github pull request"}},
    {"id": "model", "type": "provider", "config": {"provider": "anthropic"}},
    {"id": "out", "type": "output"}
  ],
  "edges": [
    {"source": "in", "target": "route"},
    {"source": "route", "target": "lane"},
    {"source": "lane", "target": "mcp"},
    {"source": "mcp", "target": "model"},
    {"source": "model", "target": "out"}
  ]
}
```

## Execution IR principles

IR should be:

- typed
- versioned
- deterministic
- validated
- serializable
- independent of React Flow
- cheap to execute

## Custom node execution

Custom nodes are externally registered node kinds. The runtime refuses to
execute them unless an `ExtensionRegistry` is installed in the
`ExecutionContext`. Extensions execute out-of-process via worker RPC —
never `dlopen` in the gateway process.

```text
Workflow JSON (CustomConfig { kind, payload })
    |
    v
ExtensionRegistry.get(kind) -> ExtensionSpec { validator, executor }
    |
    v
ExtensionExecutor.execute(config, input) -> NodeOutput
    |
    v
Worker RPC (Unix domain socket) -> External process
```

The compiler is lenient: Custom nodes pass through even if the extension
kind is not registered. Execution-time resolution is the enforcement
point. The compiler does emit a warning for unregistered kinds.

The plan hash includes the opaque `CustomConfig` payload, so any change
to the payload produces a different plan hash.

## Static validation

Compiler must detect:

- cycles where unsupported
- missing inputs/outputs
- incompatible provider capabilities
- impossible lane references
- unauthorized tools
- invalid fallback chains
- ambiguous routing
- impossible protocol translation requirements

## Runtime state

Runtime state should remain separate from immutable plan state.

```text
Plan
- what should happen

Runtime state
- what is happening now
```

This supports retries and observability without mutating the plan.
