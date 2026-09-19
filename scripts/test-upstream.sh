#!/usr/bin/env bash
# Always seed synthetic data. Never falls back to the personal Journal store.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLI="${JOURNAL_CLI:-$ROOT/target/debug/journal-rs}"
SEED=$(mktemp -d /tmp/journal-rs-fixture.XXXXXX)
trap 'rm -rf "$SEED"' EXIT
DB=$(bash "$ROOT/tests/upstream/make-fixture.sh" "$SEED")
# Populate guards that the upstream empty fixture otherwise skips.
for title in 'Native CRDT fixture' 'Synced fixture one' 'Synced fixture two'; do
  "$CLI" --db "$DB" write --body "$title" --date 2020-01-01 >/dev/null
done
sqlite3 "$DB" "update ZJOURNALENTRYMO set ZISUPLOADEDTOCLOUD=1; update ZJOURNALENTRYMO set ZMERGEABLEATTRIBUTES=X'01' where Z_PK=1;"
JOURNAL_CLI="$CLI" JOURNAL_SEED="$DB" bash "$ROOT/tests/upstream/test_write.sh"
