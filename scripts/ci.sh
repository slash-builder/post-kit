#!/usr/bin/env bash
# The gates in one place, for running before a pull request. The same checks run on GitHub Actions (.github/workflows/ci.yml).
# fmt, clippy -D warnings, tests (default and no default features), docs with -D warnings, cargo-deny, and a package dry run.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
cargo test --no-default-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --locked
if command -v cargo-deny >/dev/null 2>&1; then
  cargo deny check
else
  echo "ci: cargo-deny is not installed; install it (cargo install cargo-deny --locked) to run the supply-chain gate" >&2
  [ "${REQUIRE_DENY:-0}" = "1" ] && exit 1
fi
# A package dry run proves the crate builds from what would actually ship. Nothing is uploaded.
cargo package --locked --allow-dirty
echo "ci: all gates passed"
