# SECURITY.md — Security Model

## Threat model

The gateway handles:

- LLM API credentials
- user prompts and potentially sensitive data
- tool credentials
- MCP servers
- network routing
- arbitrary upstream endpoints
- workflow definitions

Assume tenants and external tools are untrusted relative to the gateway core.

## Security zones

```text
Public API
   |
   v
Auth/Policy
   |
   v
Control plane
   |
   +------ configuration ------+
                              |
                              v
                         Data plane
                              |
                 +------------+------------+
                 |                         |
              Providers              Sandboxed tools
```

## Secrets

Never store raw provider credentials in workflow JSON.

Use:

```text
workflow -> secret reference -> secret manager
```

Secret values must not appear in:

- logs
- traces
- metrics
- error messages
- workflow exports

## Network lanes

Every lane must have explicit:

- owner
- allowed destinations
- credentials/identity
- egress policy
- health state

A lane must not become an arbitrary SSRF primitive.

## MCP/tool permissions

Capability discovery does not imply permission to execute.

Separate:

```text
visible capability
from
executable capability
```

Policies should support allow/deny by:

- tenant
- project
- workflow
- environment
- MCP server
- tool
- endpoint

## Dynamic plugins

Do not execute arbitrary downloaded plugins in the gateway process.

Preferred model:

```text
untrusted extension
   -> sandbox/worker
   -> narrow RPC interface
   -> policy enforcement
```

## Prompt/tool injection

Treat external tool descriptions, documents, and MCP results as untrusted data.

The gateway should not infer authorization from model-generated text.

Authorization is deterministic and policy-based.

## Auditability

Record security-relevant events:

- credential reference used
- policy decision
- lane selection
- tool execution approval
- MCP connection changes
- workflow publish/version changes
- admin actions

Avoid recording sensitive payload contents by default.
