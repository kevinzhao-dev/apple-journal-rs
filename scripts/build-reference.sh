#!/usr/bin/env bash
# Build the pinned Swift reference; never access the user's Journal store.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REV=96f876e4690d5b17f4ab567aa0f7498c156213c1
DEST="$ROOT/target/upstream"
mkdir -p "$DEST"
if [[ ! -d "$DEST/source/.git" ]]; then
  git clone https://github.com/omarshahine/apple-journal-cli "$DEST/source"
fi
git -C "$DEST/source" checkout --detach "$REV"
xcrun swiftc -O -module-cache-path "$DEST/module-cache" \
  "$DEST"/source/swift/Sources/journal-cli/*.swift -o "$DEST/journal-cli"
