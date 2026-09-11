# Flow Weaver — Guidelines

## Components

The design system exports these components — import them from `@ws-hqmzuejrswrxy3onbrwn/477de595-58d4-4385-bcd3-fc6528aad873` and compose them before building anything from scratch:

`AppShell`, `Bar`, `CommandDialog`, `CommandEmpty`, `CommandGroup`, `CommandInput`, `CommandItem`, `CommandList`, `CommandPalette`, `CommandSeparator`, `CommandShortcut`, `Command`, `DialogClose`, `DialogContent`, `DialogDescription`, `DialogFooter`, `DialogHeader`, `DialogOverlay`, `DialogPortal`, `DialogTitle`, `DialogTrigger`, `Dialog`, `EmptyState`, `Inspector`, `Metric`, `NodeLibrary`, `PageHeader`, `Panel`, `RelayFlowNode`, `SectionLabel`, `StatusDot`, `StatusText`, `TableShell`, `Tag`, `Td`, `WorkflowBuilder`

Per-component details (import stanzas, props, variants, examples) live in `.lovable/rules/libraries/{slug}/components.md` — on disk, not auto-loaded. Read that file or the component source when the name alone isn't enough.

## Theme Files

The design system's theme is delivered through the following files. The author's original source files carry the full wiring the design system needs — variable declarations, framework-specific directives, provider objects, etc. — and are the canonical import target.

- `@ws-hqmzuejrswrxy3onbrwn/477de595-58d4-4385-bcd3-fc6528aad873/styles.css` (source — preferred import)
- `@ws-hqmzuejrswrxy3onbrwn/477de595-58d4-4385-bcd3-fc6528aad873/dist/tokens.css` (auto-generated flat list of CSS custom properties — a raw-values fallback only; does NOT carry framework-specific wiring that the source files above provide)

