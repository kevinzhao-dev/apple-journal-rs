# journal-rs

A Rust port of [apple-journal-cli](https://github.com/omarshahine/apple-journal-cli). Read, search, export, and edit Apple Journal entries from the terminal on macOS.

Tested on Apple Silicon with macOS 26. Writing is experimental: entries written by this tool have not been fully checked for rendering in Journal.app or syncing through iCloud.

## Install

You'll need Rust and Xcode Command Line Tools, including the Swift compiler. From a checkout of this repository:

```sh
cargo install --path . --locked
journal-rs --help
```

To access your journal, give your terminal **Full Disk Access** in **System Settings → Privacy & Security**. If you run the CLI through another app, give that app access too.

By default, it reads Apple's Journal database at:

```text
~/Library/Group Containers/group.com.apple.moments/Library/moments.sqlite
```

To use another database, pass `--db PATH` or set `JOURNAL_DB`. If both are set, `--db` wins.

## Usage

```sh
journal-rs doctor
journal-rs list --limit 10 --json
journal-rs search 'travel' --json
journal-rs show 123 --json
journal-rs journals --json
journal-rs search 'travel' --journal 2 --since 2025-01-01 --until 2026-01-01 --json
journal-rs export --dir ~/journal-export --format md
```

Replace `123` with an entry ID from `list`. `show` also reports attachment paths and whether the files are available locally. For JSON exports, use `--format json`.

Run `journal-rs --help` for all commands, or `journal-rs <command> --help` for options.

### Agent skills

This checkout includes [AGENTS.md](AGENTS.md) and three repository skills in
`.agents/skills/` for reading and reflecting on your journal:

| Skill | Example request |
| --- | --- |
| `journal-recall` | 「我最近對換工作很焦慮，過去有類似經歷嗎？後來怎麼樣？」 |
| `journal-compare` | 「比較今年和去年九月到今天，我在意的事情有什麼變化？」 |
| `journal-themes` | 「整理去年吃過、而且日記裡明確說想再訪的餐廳。」 |

Use an agent with this repository as its workspace. You can name a skill directly
(for example, `$journal-recall`) or ask a matching question. Agents without skill
discovery can read the linked `SKILL.md` files through AGENTS.md. These files are
part of the checkout; `cargo install` installs the CLI binary only.

The skills use the existing CLI, cite entry dates and IDs, and distinguish journal
evidence from interpretation. They return results in the conversation by default;
they do not edit your journal. See the [shared reading guide](docs/agent-reading.md)
for retrieval behavior and limitations. `list` and `search` accept `--journal`
with a name or numeric ID; IDs avoid ambiguity in decoded journal names.

### MCP

Run `journal-rs mcp` as a local stdio MCP server. It exposes `list_entries`, `search_entries`, `get_entry`, `list_journals`,
`write_entry`, `edit_entry`, and `export_entries`. Date and journal filters,
pagination, and entry membership IDs support the skills above. Write/edit/export
calls default to previews; explicit execution reuses the CLI's live-write guards
and exports into a new directory.

See [MCP setup and query semantics](docs/mcp.md) for client configuration and
examples. MCP date ranges use an exclusive `until`; the CLI retains its inclusive
`--until`. The server uses the same `--db` / `JOURNAL_DB` selection as the CLI.

### Writing

Start with a test database:

```sh
bash tests/upstream/make-fixture.sh /tmp/journal-demo
journal-rs --db /tmp/journal-demo/moments.sqlite \
  write --title 'Today' --body 'Coffee after a walk.'
```

Before editing your real journal, export a backup from Journal.app and quit the app. Writing to the real database requires `--live` and, the first time, `--accept-risk`. Use `--dry-run` to preview changes.

The CLI also backs up the database before a live write and applies database changes in a transaction. That backup cannot undo changes already synced to iCloud.

### Known limitations

- Chinese journal names may not decode correctly. CLI entry output does not include journal membership; MCP entry output includes numeric `journal_ids`. Changes to custom journal membership must be finalized in Journal.app to sync.
- Attachments must be available locally; the CLI does not download them from iCloud. `sandbox` copies the database without images or other external data, so it is not a complete backup.
- Dates use the local time zone. `--until 2024-12-31` ends at 00:00 on that day.
- Apple's private database format may change with OS updates. Reads copy the database and its WAL/SHM files; concurrent updates can produce an inconsistent snapshot.

## Development

Most of the code is Rust. The Swift bridge handles RTF, image resizing, and link metadata through Apple's frameworks. The commands below use generated test data; you don't need a personal journal to run them.

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
bash scripts/test-upstream.sh
```

CI runs these checks, compares the results with the original Swift CLI, and collects coverage. To run the coverage checks locally:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.9.1 --locked
bash scripts/build-reference.sh
bash scripts/coverage.sh
```

Open `target/coverage/html/index.html` for the report. Coverage does not include the Swift bridge. Image resizing tests need access to macOS ImageIO services.

## License

[MIT](LICENSE). Omar Shahine's original copyright notice is retained. This port is based on upstream commit [96f876e](https://github.com/omarshahine/apple-journal-cli/tree/96f876e4690d5b17f4ab567aa0f7498c156213c1), which is also used for compatibility tests.

This project is not affiliated with Apple.
