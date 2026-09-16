---
name: block-todo-macros
enabled: true
event: file
action: block
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.rs$
  - field: content
    operator: regex_match
    pattern: todo!\(|unimplemented!\(
---

**Forbidden: `todo!()` / `unimplemented!()` in Rust source**

This violates the project's Rust Engineering Policy (CLAUDE.md). These macros panic at runtime and indicate incomplete implementation.

**Fix immediately:**
- Replace with actual implementation
- If genuinely blocked on design, return a proper `Result::Err` with a descriptive error variant
- Never ship placeholder panics in data-plane code

No exceptions without explicit user authorization.

Enforced at `PreToolUse` with `action: block`, so a violating edit is denied before it lands. Matching runs against the `content` field, which resolves to `new_string` for `Edit`/`MultiEdit` and to `content` for `Write`; `new_text` maps to `new_string` only and would silently miss whole-file writes.
