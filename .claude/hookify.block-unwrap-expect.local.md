---
name: block-unwrap-expect
enabled: true
event: file
action: block
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.rs$
  # Mirrors check-rust-policy.sh's is_test_file(): CLAUDE.md permits .unwrap()
  # and .expect() in test files, so this rule must not fire on them.
  - field: file_path
    operator: not_contains
    pattern: test
  - field: content
    operator: regex_match
    pattern: \.unwrap\(\)|\.expect\(

---

**Forbidden: `.unwrap()` / `.expect()` in Rust source**

This violates the project's Rust Engineering Policy (CLAUDE.md). These macros panic on error and are never acceptable in production code.

(Test files are exempt — see `check-rust-policy.sh`. This rule should not have fired for one; if it did, the path lacks a `test` segment.)

**Fix immediately:**
- Replace `.unwrap()` with `?`, `.ok_or(...)`, or explicit match/if-let
- Replace `.expect("msg")` with `.context("msg")?` or `.map_err(|e| Error::...)?`
- Avoid `.unwrap_or(...)`-style shortcuts only where an error is genuinely impossible

No exceptions without explicit user authorization.

Enforced at `PreToolUse` with `action: block`, so a violating edit is denied before it lands. Matching runs against the `content` field, which resolves to `new_string` for `Edit`/`MultiEdit` and to `content` for `Write`; `new_text` maps to `new_string` only and would silently miss whole-file writes.
