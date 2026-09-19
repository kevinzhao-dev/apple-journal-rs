# Working with journal-rs

## Journal questions

Before reading personal entries, read [the journal reading guide](docs/agent-reading.md). It defines CLI access, date boundaries, evidence, and handling of personal material.

Use the relevant repository skill:

- Past experiences resembling a current situation: [.agents/skills/journal-recall/SKILL.md](.agents/skills/journal-recall/SKILL.md).
- Annual reviews, the same season in different years, or before/after comparisons: [.agents/skills/journal-compare/SKILL.md](.agents/skills/journal-compare/SKILL.md).
- Food, travel, relationships, work, or another topic across entries: [.agents/skills/journal-themes/SKILL.md](.agents/skills/journal-themes/SKILL.md).

## Repository work

Use synthetic databases for development and validation; personal journal access is unnecessary for code or documentation changes. See README.md for setup and checks, and `tests/upstream/make-fixture.sh` for fixtures.

The CLI parser is in `src/cli.rs`, read semantics in `src/read.rs`, shared filters in `src/query.rs`, MCP tools in `src/mcp.rs`, JSON records in `src/model.rs` and `src/response.rs`, and output formatting in `src/presentation.rs`. Check these when documenting commands. Run checks appropriate to the change; validate skill frontmatter and exercise documented read commands against a fixture for skill changes.
