#!/usr/bin/env bash
# rust-policy-patterns.sh — single source of truth for the Rust Engineering
# Policy forbidden-pattern regexes, shared by the PreToolUse blockers
# (block-rust-*.sh) and the policy checker (check-rust-policy.sh).
#
# Sourcing this file defines:
#   RUST_PATTERN_ALLOW_DEAD_CODE   #[allow(dead_code)] / #![allow(dead_code)]
#   RUST_PATTERN_EXPECT_DEAD_CODE  #[expect(dead_code)] / #![expect(dead_code)]
#   RUST_PATTERN_TODO              todo!()
#   RUST_PATTERN_UNIMPLEMENTED     unimplemented!()
#   RUST_PATTERN_UNWRAP            .unwrap()
#   RUST_PATTERN_EXPECT            .expect(...)
#
# Keep every pattern in one place — a policy change must never require
# touching four scripts. Patterns are POSIX ERE so they work on GNU and
# BSD/macOS grep alike: word boundaries use `(^|[^[:alnum:]_])` rather than
# GNU-only `\b` or the often-unsupported `[[:<:]]`.
# shellcheck disable=SC2034
RUST_PATTERN_ALLOW_DEAD_CODE='#!?\[allow[[:space:]]*\([^]]*dead_code'

# shellcheck disable=SC2034
RUST_PATTERN_EXPECT_DEAD_CODE='#!?\[expect[[:space:]]*\([^]]*dead_code'

# shellcheck disable=SC2034
RUST_PATTERN_TODO='(^|[^[:alnum:]_])todo![[:space:]]*\('

# shellcheck disable=SC2034
RUST_PATTERN_UNIMPLEMENTED='(^|[^[:alnum:]_])unimplemented![[:space:]]*\('

# shellcheck disable=SC2034
RUST_PATTERN_UNWRAP='\.unwrap[[:space:]]*\([[:space:]]*\)'

# shellcheck disable=SC2034
RUST_PATTERN_EXPECT='\.expect[[:space:]]*\('
