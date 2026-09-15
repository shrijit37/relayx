# MCP_SKILLS.md — Capability Discovery Architecture
> **Status:** living · **Verified:** 2026-09-16 · **Purpose:** MCP server and Agent Skills discovery design contract (Phase 7 target).

## Unified capability model

MCP tools and Skills are different primitives but share a registry/discovery lifecycle.

```text
Capability Registry
      |
      +--> MCP servers/tools
      |
      +--> Skills
      |
      +--> internal tools
```

## Lifecycle

```text
register
  -> index
  -> discover
  -> rank
  -> policy filter
  -> activate
  -> execute/use
  -> observe
```

## Retrieval design

The retrieval system should optimize for:

- recall of the correct capability
- low candidate count
- low latency
- deterministic filtering
- tenant isolation

Do not expose thousands of tool schemas solely because they exist.

## Candidate pipeline

```text
query/task context
     |
     v
lexical/metadata retrieval
     |
     v
semantic retrieval (optional)
     |
     v
policy filter
     |
     v
capability ranking
     |
     v
small candidate set
     |
     v
schema activation
```

## Important distinction

```text
Discovery != Authorization
Activation != Execution permission
Execution permission != successful execution
```

## Failure handling

Retrieval failure must be observable.

Record:

- query
- registry version
- candidate count
- selected capability
- retrieval latency
- cache status
- no-match result

Avoid logging sensitive task text unless explicitly configured.

## Progressive Skill loading

Recommended sequence:

```text
metadata
  -> SKILL.md/instructions
  -> selected references
  -> selected scripts/resources
```

Large reference material should remain out of context until required.

## Evaluation

Maintain a benchmark corpus with:

- known correct tool
- distractor tools
- ambiguous descriptions
- near-duplicate tools
- missing-tool cases
- authorization-denied cases

Metrics:

- recall@k
- precision@k
- activation accuracy
- task success rate
- tokens added
- retrieval latency
