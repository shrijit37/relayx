---
name: block-unwrap-expect
enabled: true
event: file
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.rs$
  # Mirrors check-rust-policy.sh's is_test_file(): CLAUDE.md permits .unwrap()
  # and .expect() in test files, so this rule must not fire on them.
  - field: file_path
    operator: not_contains
    pattern: test
  - field: new_text
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
