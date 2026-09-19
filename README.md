# journal-rs

A macOS command-line tool for reading, searching, exporting, and editing Apple Journal entries. A Rust port of [apple-journal-cli](https://github.com/omarshahine/apple-journal-cli).

Experimental. Tested on Apple Silicon with macOS 26. Journal.app rendering and iCloud sync after writes have not been fully verified.

## Install

Requires Rust and Xcode Command Line Tools with the Swift compiler. From the repository directory:

```sh
cargo install --path . --locked
journal-rs --help
```

Grant your terminal access under **System Settings → Privacy & Security → Full Disk Access**. If you run the CLI through another app, that app also needs permission.

Default database:

```text
~/Library/Group Containers/group.com.apple.moments/Library/moments.sqlite
```

Use `--db PATH` or `JOURNAL_DB` to select another database. `--db` takes precedence.

## Usage

```sh
journal-rs doctor
journal-rs list --limit 10 --json
journal-rs search 'travel' --json
journal-rs show 123 --json
journal-rs journals --json
journal-rs export --dir ~/journal-export --format md
```

`show` lists attachment paths and whether the files exist. `export` supports Markdown and JSON. Other commands include `stats`, `deleted`, `sandbox`, `write`, `edit`, `delete`, `restore`, `empty`, `repair-locations`, `sync-journals`, and `render`. Run `journal-rs <command> --help` for options.

### Writing

Try a synthetic database first:

```sh
bash tests/upstream/make-fixture.sh /tmp/journal-demo
journal-rs --db /tmp/journal-demo/moments.sqlite \
  write --title 'Today' --body 'Coffee after a walk.'
```

Before editing your real journal, export a backup from Journal.app and quit the app. Writes require `--live`, plus `--accept-risk` on the first use. All commands that modify entries support `--dry-run`. The CLI backs up the database and uses SQL transactions, but a local backup cannot undo changes already synced to iCloud.

### Known limitations

- Chinese journal names may not decode correctly. Entry output does not yet include journal membership. Changes to custom journal membership must be finalized in Journal.app to sync.
- Attachments must be available locally; the CLI does not download them from iCloud. `sandbox` copies the database without images or other external data, so it is not a complete backup.
- Dates use the local time zone. `--until 2024-12-31` ends at 00:00 on that day.
- Apple's private database format may change with OS updates. Reads copy the database and its WAL/SHM files; concurrent updates can produce an inconsistent snapshot.

## Development

The core is written in Rust. A small Swift bridge handles Apple's RTF, image resizing, and link metadata APIs. Tests use synthetic data and do not require a personal journal.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
bash scripts/test-upstream.sh
```

CI also compares behavior against upstream and measures coverage. To generate a report locally:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.9.1 --locked
bash scripts/build-reference.sh
bash scripts/coverage.sh
```

The HTML report is at `target/coverage/html/index.html` and excludes the Swift bridge. Image resizing tests need access to macOS ImageIO services.

## License

[MIT](LICENSE), with Omar Shahine's original copyright notice retained. The port and compatibility tests use upstream commit [96f876e](https://github.com/omarshahine/apple-journal-cli/tree/96f876e4690d5b17f4ab567aa0f7498c156213c1) as their reference. This project is not affiliated with Apple.
