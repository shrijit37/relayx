---
name: require-clippy-before-stop
enabled: true
event: stop
pattern: \.rs$
---

**Stop check: Rust quality gates not verified**

Before stopping, run these commands in a clean state:

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo test --workspace
```

The project's Rust Engineering Policy requires all three to pass. Do not claim completion without running them.

If any check fails, fix the issues before reporting done.
