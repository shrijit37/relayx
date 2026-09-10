# PROTOCOLS.md — Protocol Translation Contract

## Goal

Provide reliable interoperability between client-facing and upstream LLM APIs without silently degrading functionality.

## Supported protocol families

Initial target:

- Anthropic Messages API
- OpenAI Chat Completions
- OpenAI Responses

Future:

- Gemini/Google APIs
- local model APIs
- other OpenAI-compatible providers

## Translation principles

### Lossless by default

Every adapter should answer:

1. What input information arrives?
2. Where is it represented in canonical form?
3. How is it emitted downstream?
4. What cannot be represented?
5. How is that limitation reported?

### Canonical model

Use typed internal concepts such as:

```text
Request
Message
ContentBlock
ToolDefinition
ToolCall
ToolResult
ResponseEvent
Usage
Error
CapabilityReference
```

Avoid using an untyped map as the internal contract.

## Deferred MCP / tool references

Tool discovery mechanisms can expose references rather than full schemas. These must not be flattened accidentally.

The adapter pipeline must distinguish:

```text
fully_loaded_tool
vs
referenced/deferred_tool
```

A compatibility layer must either:

- preserve the reference semantics end-to-end, or
- intentionally resolve it before sending downstream, with explicit capability accounting.

Never silently discard a reference.

## Streaming

Adapters must translate streaming events incrementally.

Tests must cover:

- text deltas
- tool-call deltas
- tool-result messages
- usage events
- end-of-stream
- error after partial stream
- cancellation
- client disconnect

## Provider-native extensions

Use an extension namespace for information that is not part of the common core.

```text
extensions:
  anthropic: {...}
  openai: {...}
  provider_x: {...}
```

Adapters may support only a declared subset.

## Capability matrix

Every provider adapter should expose machine-readable capabilities:

```text
supports_streaming
supports_tools
supports_structured_output
supports_reasoning
supports_cache_hints
supports_deferred_tools
supports_images
supports_audio
supports_citations
```

Routing and workflow compilation may reject incompatible plans before runtime.

## Error semantics

Expose stable gateway-level error categories while retaining upstream metadata.

```text
translation_error
unsupported_feature
provider_rejection
rate_limit
authentication
network
cancelled
```

Do not convert a provider feature mismatch into a generic 500 when a precise error is possible.
