---
name: block-unwrap-expect
enabled: true
event: file
conditions:
  - field: file_path
    operator: regex_match
    pattern: \.rs$
  - field: new_text
    operator: regex_match
    pattern: \.unwrap\(\)|\.expect\(

---

**Forbidden: `.unwrap()` / `.expect()` in Rust source**

This violates the project's Rust Engineering Policy (CLAUDE.md). These macros panic on error and are never acceptable in production code.

**Fix immediately:**
- Replace `.unwrap()` with `?`, `.ok_or(...)`, or explicit match/if-let
- Replace `.expect("msg")` with `.context("msg")?` or `.map_err(|e| Error::...)?`
- For tests: use `assert!` / `assert_eq!` or `#[should_panic]` — still no `.unwrap()`

No exceptions without explicit user authorization.
