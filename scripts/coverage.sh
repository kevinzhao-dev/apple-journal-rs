#!/usr/bin/env bash
# Include instrumented CLI subprocesses, upstream tests, and differential checks.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
REFERENCE="${1:-$ROOT/target/upstream/journal-cli}"
if [[ ! -x "$REFERENCE" ]]; then
  echo 'Build the reference with bash scripts/build-reference.sh or pass its absolute path.' >&2
  exit 1
fi
export CARGO_TARGET_DIR="$ROOT/target/coverage-build"
cargo llvm-cov clean --workspace
# Environment emitted by the installed coverage tool, not external input.
COVERAGE_ENV="$(cargo llvm-cov show-env --sh)"
eval "$COVERAGE_ENV"
cargo test --locked
cargo build --locked
CLI="$CARGO_LLVM_COV_TARGET_DIR/debug/journal-rs"
JOURNAL_CLI="$CLI" bash scripts/test-upstream.sh
python3 scripts/compare-upstream.py "$REFERENCE" --rust "$CLI"
REPORT="$ROOT/target/coverage"
mkdir -p "$REPORT"
COMMON=(--ignore-filename-regex '/tests/|/build\.rs$')
cargo llvm-cov report "${COMMON[@]}" --show-missing-lines > "$REPORT/summary.txt"
cargo llvm-cov report "${COMMON[@]}" --html --output-dir "$REPORT"
cargo llvm-cov report "${COMMON[@]}" --lcov --output-path "$REPORT/lcov.info"
cargo llvm-cov report "${COMMON[@]}" --json --summary-only --output-path "$REPORT/summary.json"
python3 scripts/coverage-summary.py "$REPORT/summary.json" > "$REPORT/summary.md"
cat "$REPORT/summary.md"
