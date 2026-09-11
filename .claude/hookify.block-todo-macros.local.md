---
name: block-todo-macros
enabled: true
event: file
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.rs$
  - field: new_text
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
