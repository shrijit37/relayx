# Ponytail Audit — Over-engineering Scan
> **Archived snapshot** — historical record, not current truth. Current state: [../state.md](../state.md).

Date: 2026-09-16 · Command: `/ponytail:ponytail-audit` · Scope: whole tree, complexity only (correctness/security/performance out of scope).

Ranked biggest cut first.

## Findings

| # | tag | what to cut | replacement | path |
|---|-----|-------------|-------------|------|
| 1 | `delete:` | 36 of 46 shadcn/ui wrapper components (only `button`, `command`, `dialog`, `input`, `label`, `separator`, `sheet`, `skeleton`, `toggle`, `tooltip`, `sonner` are reachable) | nothing | `apps/web/src/components/ui/*` |
| 2 | `delete:` | 29 unused frontend deps (20 dead radix + 9 wrapped: `input-otp`, `react-day-picker`, `embla-carousel-react`, `vaul`, `react-resizable-panels`, `react-hook-form`, `@hookform/resolvers`, `date-fns`, `recharts`) | nothing | `apps/web/package.json` |
| 3 | `delete:` | dead metric fns `record_bytes_in/out`, `record_upstream_body_duration`, `track_active_connection` (zero callers) | nothing | `apps/gateway/src/observability/mod.rs:180-205` |
| 4 | `delete:` | `scripts/demo-phase6.sh` | CP tests already boot pool + Fastify | `scripts/` |
| 5 | `yagni:` | `RuntimeSnapshotBuilder` (17-line growing config object, test-only) | `RuntimeSnapshot::empty(wall_version)` | `crates/workflow-runtime/src/snapshot.rs:113` |
| 6 | `yagni:` | `RouterStrategy::LoadBased` (unimplemented: round-robin until load metrics exist) | drop the variant | `crates/workflow-runtime/src/nodes/router.rs:34` |
| 7 | `delete:` | `NodeRegistry`/`NodeExecutor` extension seam (zero registered nodes in repo) | trait dispatch only when a real extension exists | `crates/workflow-runtime/src/nodes/trait_node.rs` |
| 8 | `shrink:` | `Capabilities::excess/is_satisfied_by/from_protocol` hand-rolled subset-checking over 9 bools | one loop over `const` field list | `crates/workflow-runtime/src/capability.rs:31-56` |
| 9 | `stdlib:` | `send_request_with_timeout` (hand-rolled select! timeout wrapper) | `tokio::time::timeout(client.request(req))` | `crates/workflow-runtime/src/nodes/llm.rs:202` |
| 10 | `shrink:` | `GatewayHttpClient` alias + 3 identical hyper client builders | one shared constructor per crate | `crates/workflow-runtime/src/context.rs:19`, `apps/gateway/src/lanes.rs` |
| 11 | `yagni:` | `plan_hash_for` + `workflow_version_for` side maps written/read together | single map to a small struct | `crates/workflow-runtime/src/snapshot.rs:31-36` |
| 12 | `delete:` | duplicated gateway test helpers (`publish_*`/`validate_workflows` mirrors, SSE scavenging re-tested in `admin_stream_sse.rs`) | keep boundary tests only | `apps/gateway/tests/*` |
| 13 | `delete:` | `fetchCatalogLogo` + `catalog_status`/`catalog_providers` fetch + card in providers.tsx | URL-based `<img>` loading | `apps/web/src/routes/providers.tsx`, `apps/web/src/lib/api.ts:454-467` |
| 14 | `shrink:` | `fromWorkflowJson` re-run every keystroke in `WorkflowBuilder` | debounce / diff on `workflow.version` | `apps/web/src/components/relay/workflow/WorkflowBuilder.tsx` |
| 15 | `yagni:` | `domain/wire.ts` hand mirror of gateway serde `Wire*` types | single source of truth (generate or import) | `apps/control-plane/src/domain/wire.ts` |
| 16 | `delete:` | 4 parallel hook/lint toolchains shipped and stored but not all executed | one active linter | `.claude/hooks`, `.codex/hooks`, `.forge/skills`, `.opencode/hook` |
| 17 | `delete:` | checked-in `routeTree.gen.ts` (router plugin regenerates it) | generate in CI, ignore in git | `apps/web/src/routeTree.gen.ts` |
| 18 | `shrink:` | `with_plan_version` + `workflow_versions` second map (version stored twice) | read from plan itself | `crates/workflow-runtime/src/snapshot.rs`, `context.rs:64` |
| 19 | `stdlib:` | hand-rolled delay/`AbortSignal.timeout` periodic timers | system task schedulers | gateway/control-plane |
| 20 | `yagni:` | 8 skills cloned under 3 tool-homes (`.agents/`, `.forge/`, `.claude/`) | keep only the 2 real project skills tracked in git | `.agents/skills`, `.forge/skills` |

## Where it stays lean

Do NOT cut (honest architecture, actively used):

- `ArcSwap` snapshot/pool publication (doubled-load pattern is correct by design)
- protocol adapters' conformance tests
- `protocol-core` `Protocol` enum reuse
- `mock-upstream` / `test-harness` (real reusable test infra)
- `workflow-schema` + `compiler` split
- `004_runs.sql` migration (consumed by `repo.runs` / `routes/runs.ts`)

## Net

`-4,000+ lines, -29 deps possible`

(≈3,800 lines of dead UI wrappers + ≈200 Rust lines across 4 crates + ≈150 script/hook/skill lines; 20 radix + 9 wrapped deps)
