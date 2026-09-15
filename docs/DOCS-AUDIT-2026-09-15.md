# Documentation Audit — 2026-09-15

## Verified Counts (Today)

| Component | Actual | state.md | README.md | testing.md |
|---|---|---|---|---|
| **Rust workspace** | **294** | 286 | 270 | 131 (partial) |
| workflow-schema | 16 | 12 | — | 12 |
| workflow-runtime | 64 | 52 | — | 52 |
| protocol-core | 133 | — | — | 131 |
| **Frontend** | **31** | 31 ✅ | — | varies |
| **Control-plane** | **16** | 16 ✅ | — | — |
| **Page routes** | **15** | 15 ✅ | — | — |

## Per-File Findings

### README.md — ⚠️ Partially stale

- **Line 46**: "Rust tests | 270 passing" → actual **294**
- **Lines 69-80**: Project structure omits `apps/control-plane/` entirely. This is the largest TypeScript component in the repo (Fastify + PostgreSQL, 16 tests).
- **Lines 69-80**: Omits `crates/workflow-runtime/` from the tree — exists with 64 tests.

### CLAUDE.md — ⚠️ Partially stale

- **Lines 156-170**: Repository structure omits `apps/control-plane/` and `crates/workflow-runtime/`. Both exist and are significant.
- Skills path references `.claude/skills/` — actual directory is `.agents/skills/`.

### AGENTS.md — ⚠️ Partially stale

- Same repo structure gap as CLAUDE.md (missing control-plane, workflow-runtime from tree).

### docs/state.md — ❌ Significantly stale (critical source-of-truth doc)

- **Line 20/148**: "Rust 286 passing" → actual **294** (off by 8)
- **Line 92**: workflow-schema "12 tests passing" → actual **16** (12 unit + 4 wire_compat)
- **Line 100**: workflow-runtime "52 tests" → actual **64** (35 lib + 6 extension + 2 integration + 19 protocol + 2 context)
- Routes count (15) and frontend count (31) are correct.

### docs/testing.md — ⚠️ Partially stale

- **Line 107**: protocol-core "131 tests" → actual **133**
- **Line 141**: workflow-schema "12 tests" → actual **16**
- **Line 143**: workflow-runtime "52 tests" → actual **64**
- Phase 1 gateway unit count (17) is slightly under — actual is 22.

### docs/development.md — ⚠️ Partially stale

- Phase annotations outdated: `apps/gateway/` described as "Phase 1 complete" — it's through Phase 6.5+.
- `crates/workflow-runtime/` described as "Phase 4/5 complete" — now also Phase 6.5 (auth, deadline, frame_timeout wiring).

### docs/roadmap.md — ⚠️ Partially stale

- No mention of Phase 6.6 (canonical workflow model), which is completed per PHASE6.6_REPORT.md.
- Phase numbering slightly confusing around 5b/6.

### docs/CURRENT_STATE.md — 🗑️ Superseded (self-declared)

- Line 1: "Superseded by Phase 5" — honest, but should be deleted or marked with "DO NOT REFERENCE".
- Claims "197 tests", "control plane does not exist", "workflow-runtime: 0 tests" — all false.
- Risk: new team members may find this file and trust it.

### docs/architecture.md — ✅ Current

- Correctly labeled as "target architecture" with explicit caveats about what's implemented vs. planned. No corrections needed.

### docs/performance.md — ✅ Current

- Benchmark methodology, hot-path rules, and baseline measurements accurate.

### docs/observability.md — ✅ Current

- Metric names, labels, and admin endpoints accurate.

### docs/security.md — ✅ Current (design doc)

- Threat model and security boundaries accurate as design guidance.

### docs/protocols.md — ✅ Current (design doc)

- Translation contract description matches implementation.

### docs/workflow-ir.md — ✅ Current (design doc)

### docs/mcp-skills.md — ✅ Current (design doc, future-state)

### docs/adr-0001-stack.md through adr-0004-control-data-plane.md — ✅ Current

- All four ADRs accurately reflect ongoing architectural decisions.

### Phase Reports (historical snapshots)

| File | Status | Note |
|---|---|---|
| docs/phase-1-2-report.md | ✅ Historical | Accurate for its date |
| docs/PHASE5_REPORT.md | ✅ Historical | Accurate for Phase 5 |
| docs/PHASE6_REPORT.md | ✅ Historical | Accurate for Phase 6 |
| docs/PHASE6.5_IMPLEMENTATION_REPORT.md | ✅ Historical | Accurate for Phase 6.5 |
| docs/PHASE6.5_REPORT.md | ✅ Historical | Accurate for Phase 6.5 hardening |
| docs/PHASE6.6_REPORT.md | ⚠️ Stale | "Known limitations" section says toolbar buttons are stubs — they are now wired (Save/Validate/Publish all have real onClick handlers). |

### apps/web/src/routes/README.md — ✅ Current

- TanStack Start routing conventions accurate.

## Recommended Actions

### P0 — Actively misleading

1. **`docs/state.md` line 20/148**: Change "286" → "294". Anyone using this to gauge test coverage is misled.
2. **`README.md` line 46**: Change "270" → "294".

### P1 — Stale claims affecting onboarding

3. **`README.md` project structure**: Add `apps/control-plane/` and `crates/workflow-runtime/` to the tree.
4. **`CLAUDE.md` repo structure**: Same addition.
5. **`docs/state.md` workflow-schema**: "12 tests" → "16".
6. **`docs/state.md` workflow-runtime**: "52 tests" → "64".
7. **`docs/CURRENT_STATE.md`**: Delete or add `# DO NOT REFERENCE — superseded by Phase 5` header.
8. **`docs/testing.md`**: protocol-core "131" → "133", workflow-schema "12" → "16", workflow-runtime "52" → "64".

### P2 — Confusing or slightly wrong

9. **`docs/development.md`**: Update phase annotations for gateway and workflow-runtime.
10. **`docs/roadmap.md`**: Add Phase 6.6 as completed milestone.
11. **`docs/PHASE6.6_REPORT.md`**: Update "known limitations" section — toolbar buttons are wired.
12. **`CLAUDE.md`**: Fix skills path from `.claude/skills/` to `.agents/skills/`.
