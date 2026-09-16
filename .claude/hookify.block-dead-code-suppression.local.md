---
name: block-dead-code-suppression
enabled: true
event: file
action: block
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.rs$
  - field: content
    operator: regex_match
    pattern: #\[allow\(dead_code\)\]|#!\[allow\(dead_code\)\]|#\[expect\(dead_code\)\]|#!\[expect\(dead_code\)\]
---

**Forbidden: Dead-code suppression in Rust source**

This violates the project's Rust Engineering Policy (CLAUDE.md). Hiding dead code with `#[allow(dead_code)]` or `#[expect(dead_code)]` is never permitted.

**Fix the underlying issue instead:**
1. If code is unused → remove it
2. If code will be used soon → add the usage first, then the code
3. If the compiler is wrong (false positive) → narrow the suppression to the specific item with a comment explaining why, and get explicit user authorization

Item-scoped `#[allow(...)]` for FFI, generated code, or platform-specific code is permitted with a documented reason.

Enforced at `PreToolUse` with `action: block`, so a violating edit is denied before it lands. Matching runs against the `content` field, which resolves to `new_string` for `Edit`/`MultiEdit` and to `content` for `Write`; `new_text` maps to `new_string` only and would silently miss whole-file writes.
