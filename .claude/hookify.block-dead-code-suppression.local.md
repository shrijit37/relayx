---
name: block-dead-code-suppression
enabled: true
event: file
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.rs$
  - field: new_text
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
