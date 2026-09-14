# PHASE 6.6 REPORT — Canonical Workflow Model & Lossless Editor Architecture

**Status:** Implemented (with fixes from code review) · **Branch:** `feat/phase6.5-review`

## 1. What changed

Relay-X now has **one canonical semantic workflow model** (`apps/web/src/lib/workflow/`). React Flow is a view over it; persistence, validation, and the Inspector all operate on the same model. The previous lossy boundary — a handwritten serializer inferring provider/model/lane from display titles and silently dropping unsupported nodes — is gone.

The `/code-review` pass on the WIP found three correctness bugs (silent error drop, wrong serialize shape, condition `Value` field hidden) plus a structural defect: a legacy adapter (`workflow-serializer.adapter.ts`) re-implemented view→schema instead of delegating to the canonical serializer, so two kind-mapping tables could drift and condition-branch edges lost their ports on load. All were fixed; the legacy adapter and shim were deleted.

## 2. Canonical workflow model

`apps/web/src/lib/workflow/nodes.ts` defines the single `CanonicalWorkflow`:

```text
Workflow
├── id · name · version · schemaVersion
├── nodes[]
│   ├── id · type (EditorKind) · version
│   ├── position {x,y}            — explicit, stored
│   ├── ports[]                   — typed (name, direction, portType, cardinality)
│   ├── config                    — per-kind typed CanonicalConfig
│   └── presentation              — title/notes ONLY (never parsed)
└── edges[]
    ├── id
    ├── source/target + explicit sourcePort/targetPort
    └── label                     — presentation (e.g. condition "true"/"false" via ports)
```

Canonical config is explicit and typed — `llm.config = {provider, model, lane, temperature, maxTokens, stream}`. A display title can never change runtime behavior (§3.1 §3.4). Node instances hold values; `NodeDefinition` (node-definitions.ts) describes what those values mean (defaults, options, validation rules, ports, `executable`). Display-only kinds (`policy`, `lane`, `tool`, `agent`, `endpoint`, `observability`) have `executable: false` — they serialize as explicit `unsupported` nodes and **block publish** rather than vanishing (§3.2).

## 3. Node-definition architecture

`node-definitions.ts` — every `EditorKind` maps to a `NodeDefinition`:

```text
NodeDefinition
├── type / schemaVersion / label
├── inputs[] / outputs[]       — PortDef (direction, portType, required, cardinality)
├── fields[]                   — FieldDef (name, label, type, required, default, min/max, options, reference, dependsOn)
├── defaults()                 — typed default config for new nodes
├── displayTitle()/displayLines() — presentation derived FROM config, never the reverse
└── executable / note
```

`dependsOn` supports `|`-separated value lists (e.g. condition `value` shown only for the operators that need it). The Inspector renders `FieldDef`s into a typed form.

## 4. Serializer / deserializer changes

`apps/web/src/lib/workflow/serializer.ts` (the single serializer):

- **Lossless round-trip:** `deserialize(serialize(workflow))` preserves all executable semantics — explicit positions, typed config, ports, condition branches. Edge `source_port`/`target_port` become `sourceHandle`/`targetHandle`, condition edges keep their branch identity.
- **Rust wire contract enforced:** emitted `condition` configs now match `workflow_schema::ConditionConfig` (`condition` string + snake_case `equal`/`not_equal` operator vocab, mapped from the editor's `equals`/`not_equals` via `CONDITION_OP_TO_RUST`), and `fallback` providers carry the required `model` override (`FallbackProvider.model`). Verified by `crates/workflow-schema/tests/wire_compat.rs` — parses the exact web-emitted JSON.
- **No inference, no fabrication, no silent drops:** unknown/unmappable persisted kinds produce a load-time `error` (never a crash, never a silent skip). Missing required config (lane, model, condition field/value, fallback model) blocks `serializeWorkflow` (`workflow: null` + errors). A workflow with no Input/Output node refuses to serialize.
- **Deterministic migration:** legacy v1 JSON (no `schema_version`, no positions) gets a stable 3-column layout on load (off-by-one fixed — y advances only every third node); `schema_version` upgrades on next save. Synthesized edge ids are unique (`e-${index}`).
- **Dead code eliminated:** the old `workflow-serializer.adapter.ts` (its own `KIND_TO_SCHEMA`, `schemaConfigOf`, `canonicalFromView`) and the `workflow-serializer.ts` shim were **deleted**. `api.ts`, `use-workflow-publication.ts`, `WorkflowBuilder.tsx` now import `@/lib/workflow` directly.

## 5. Inspector changes

`Inspector.tsx` is a real schema-driven editor (§8):

- Select node → `fromViewNode` → canonical → `NodeDefinition.fields` → typed form → `onConfigChange` writes back to the canonical config carried on the view (`data.canonicalConfig`).
- **Title is display-only:** editing it writes `presentation.title` only, never config.
- **Condition `Value` field bug fixed:** the `dependsOn` filter now splits its value on `|` and tests membership, so `equals`/`contains`/etc. show the field; it was previously hidden for every operator.
- Reference fields (lane/provider/model) with resolved options render as dropdowns; without options they fall back to free-text input (never an empty select).
- Live per-node issues render under the config (§15): missing required, invalid enum, unknown lane reference.

## 6. Validation changes

`lib/workflow/validation.ts` — layered (§14):

- **Structural:** unique ids, known ports, valid node types, acyclicity, Input/Output presence. Editor-local, runs per change.
- **Schema:** required fields (treats `0`/`false` as present — not falsy-empty), types, enums, ranges.
- **Semantic:** lane/model/provider references checked against the control-plane lane table (via `useLanes()`).
- Backend/compile remains authoritative for publishability (§16) — unchanged.

`WorkflowBuilder` now computes a canonical model with `fromViewNode`/`toCanonicalEdges` and runs `validateWorkflow` live, passing `issues` + `laneOptions` into the Inspector. Load-time errors/warnings from `deserializeWorkflow` surface as toasts — an unloadable node is never silently dropped.

The serializer's publish gate is **not** inspector-only: `toWorkflowJson` runs `validateStructural` (Input/Output presence, cycles, port existence) plus per-node schema gates (condition field/value, fallback model) on every serialize — so an input-less or unconfigured-condition workflow returns `workflow: null` with errors, matching the old behavior the pre-WIP suite enforced.

## 7. Migration strategy

- `schema_version` omitted / `< 2` → migrated deterministically on load (stable positions, no title/topology parsing).
- Stored **schema** kinds (`llm`, `router`, `mcp`, …) map to **editor** kinds (`provider`, `route`, …) through `typeFromSchema`; unknown kinds are refused with a clear error.
- Warnings/errors are explicit; ambiguous legacy fields are reported, never invented.

## 8. Tests added

`apps/web/src/lib/workflow-serializer.test.ts` (+ 6 new):
- Condition edge keeps explicit `true`/`false` port through save→load (both `sourceHandle` and `targetHandle`).
- Condition numeric value round-trips as a number, not a string.
- `serialize(deserialize(serialize(x)))` idempotent for LLM config + positions.
- Persisted `llm` kind loads as editor `provider` carrying its config (the crash class).
- Unknown persisted kinds are refused with an error, never fabricated nodes.
- Existing round-trip/negative/title-inference/migration/version tests continue to pass.

## 9. Existing tests / results

```
apps/web:  bun test → 23 pass / 0 fail · 64 expect() calls
           tsc --noEmit → 0 errors, 0 warnings
           vite build → clean

crates/workflow-schema:  wire_compat → 4 pass (llm/condition/fallback/full-snapshot serde)
                         full workspace → 162 pass / 0 fail
```

Rust wire-compat tests (`crates/workflow-schema/tests/wire_compat.rs`) prove the web serializer's emitted JSON parses through the Rust `workflow_schema` crate — no schema-contract drift is possible.

## 10. Known limitations

- `Run test` panel and Undo/Redo toolbar buttons are non-functional stubs (Phase 6.5 scope — flagged in review, intentionally deferred, not regressed here).
- Publish/compile still requires the live control-plane+gates wiring (Phase 6.5) — local validation is surface-level; the backend remains authoritative.
- The view node's `canonicalConfig` is the inspector's source of truth; newly drag-added nodes start from the typed default until configured.

## 11. Deferred execution-engine work

No execution-engine changes in this phase. Deterministic port selection, multi-value fan-in/fan-out semantics, node-level error policies (retry/fallback are already modeled; generalized `continueOnFail`-style semantics are not) remain future work — see `docs/workflow-ir.md`.

## 12. Performance impact

**None to the data plane.** All changes are editor/control-plane-side. The runtime still consumes the immutable compiled `ExecutionPlan`; no DB/control-plane calls, no schema discovery, no title parsing, and no extra JSON cycles were added to the request path. Serializer runs only on explicit Save/Validate/Publish.

## 13. Security implications

None new. The serializer never parses titles into semantics; unknown/policy/adversarial persisted shapes degrade to explicit errors (no code paths enabled). No new trust boundaries introduced (no secrets, no network lanes touched).

## 14. Files changed

| File | Change |
|---|---|
| `apps/web/src/lib/workflow/nodes.ts` | Canonical model; `FallbackEntryConfig.model` added (Rust `FallbackProvider.model` contract) |
| `apps/web/src/lib/workflow/node-definitions.ts` | `dependsOn` `|` contract; condition `Value` field visible for ops that need it |
| `apps/web/src/lib/workflow/serializer.ts` | `serializeWorkflow`/`deserializeWorkflow` public API; `CONDITION_OP_TO_RUST`/`RUST_OP_TO_EDITOR` maps; condition emits `condition` string + snake_case operator; fallback validates model; `toWorkflowJson` runs `validateStructural`; unique edge ids; `errors` on deserialize |
| `apps/web/src/lib/workflow/validation.ts` | Dead stubs removed; `validateConfig` uses `issueFor`; retry target lane validated |
| `apps/web/src/lib/workflow/index.ts` | Doc: single public surface |
| `apps/web/src/lib/workflow-serializer.adapter.ts` | **Deleted** (superseded) |
| `apps/web/src/lib/workflow-serializer.ts` | **Deleted** (legacy shim) |
| `apps/web/src/lib/workflow-serializer.test.ts` | 23 tests (gate/edge/condition/serde contracts); import `@/lib/workflow` |
| `apps/web/src/lib/api.ts` | Import `@/lib/workflow` |
| `apps/web/src/lib/use-workflow-publication.ts` | Import `@/lib/workflow` |
| `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx` | Canonical serialize/load via `@/lib/workflow`; live validation + lanes wired; `toWorkflowJson` structural gate; `inspectorCanonical = fromViewNode` |
| `apps/web/src/components/relay/workflow/Inspector.tsx` | `dependsOn` split; fallback `rounds` coercion fix; reference fields fall back to text input; wired `issues`/`laneOptions` |
| `apps/web/src/components/relay/workflow/nodes.tsx` | Renderer guard `nodeMeta[data.kind] ?? nodeMeta.tool` |
| `crates/workflow-schema/tests/wire_compat.rs` | **New** — serde round-trip proof that web-emitted JSON parses through `workflow_schema` |

Invariant preserved: **one semantic model** — editor, persistence, validation, and compiler all operate on the same canonical workflow.