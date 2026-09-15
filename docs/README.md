# docs/ — Documentation Index
> **Status:** living · **Verified:** 2026-09-16 · **Purpose:** Documentation conventions plus the index agents and humans read first.

## Read this first

If you are an agent or a new contributor, read in this order:

1. [`../AGENTS.md`](../AGENTS.md) — how to build, test, and change the repo
2. [`state.md`](state.md) — what is actually implemented today (canonical facts)
3. the specific doc below for the area you are touching
4. [`adr-0001-stack.md`](adr-0001-stack.md) — why the architecture is the way it is

## Document map

| Doc | Answers | Update it when |
| --- | --- | --- |
| [`state.md`](state.md) | What works today, phase status, test counts, known gaps | Any behavior, phase, or test-count change |
| [`architecture.md`](architecture.md) | Target topology + domain model (implemented vs. planned) | A boundary or component relationship changes |
| [`development.md`](development.md) | Repo layout, local setup, CI | Tooling, layout, or pipeline changes |
| [`testing.md`](testing.md) | Test strategy and categories | Test categories or suites change |
| [`roadmap.md`](roadmap.md) | Phase 0–9 checklist | A roadmap item starts or completes |
| [`protocols.md`](protocols.md) | Translation contract + capability loss | An adapter or translation rule changes |
| [`workflow-ir.md`](workflow-ir.md) | Workflow model / execution IR | Schema or compiler changes |
| [`performance.md`](performance.md) | Budget, benchmarks, hot-path rules | A perf-sensitive path or benchmark changes |
| [`observability.md`](observability.md) | Metrics, logging, tracing contract | A metric, label, or endpoint changes |
| [`security.md`](security.md) | Threat model + boundaries | Auth, secrets, or permissions change |
| [`mcp-skills.md`](mcp-skills.md) | MCP/Skills discovery design | Discovery design changes |
| [`adr-*.md`](adr-0001-stack.md) | Why a decision was made | A decision is accepted, superseded, or reversed |
| [`archive/`](archive/) | Historical snapshots (never current truth) | Never — snapshots are immutable |

## Conventions

### 1. Every doc is `living` or `snapshot`

Living docs — everything in `docs/` except `archive/` — start with a header:

```text
> **Status:** living · **Verified:** YYYY-MM-DD · **Purpose:** one line
```

`Verified` is the last date a human or agent confirmed the doc against the code.
Snapshots live in [`archive/`](archive/) and carry a "not current truth" banner.
ADRs are exempt from the header: they carry a `## Status` section instead.

### 2. One canonical fact, one place

Test counts, page-route counts, and similar numbers are computed from the code
and written once — the canonical-facts block in [`state.md`](state.md). Never
hand-copy a number into another doc without it being checked: quote it and let
`scripts/verify-docs.sh` verify it, or link to `state.md`.

### 3. Snapshots are immutable

Phase reports and audits are historical records. Do not edit them to "fix"
staleness — write a new living doc instead. When a phase completes, move its
evidence into `archive/`.

### 4. ADRs are append-only

An ADR that no longer applies is marked `## Status→ Superseded by ADR-XXXX`,
not deleted or silently rewritten.

## Enforcing truth

```bash
scripts/verify-docs.sh          # fail when docs disagree with the code
scripts/verify-docs.sh --write  # regenerate the canonical-facts block
```

The check runs in three places: the Claude Code `PostToolUse` hook
(`.claude/hooks/check-docs.sh`), the git pre-commit hook
(`.githooks/pre-commit`), and the CI `docs` job (`.github/workflows/ci.yml`).