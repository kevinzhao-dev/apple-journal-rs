//! Terminal/JSON formatting; operations never print directly.
use crate::{
    cli::Command,
    model::{AssetDetails, Entry},
    response::*,
};
use anyhow::Result;
use std::fmt::Write as _;
fn json(value: &impl serde::Serialize) -> Result<Vec<u8>> {
    let mut data = serde_json::to_vec_pretty(value)?;
    data.push(b'\n');
    Ok(data)
}
fn head(e: &Entry) -> String {
    if !e.title.is_empty() {
        e.title.clone()
    } else {
        e.text
            .lines()
            .next()
            .unwrap_or("(empty)")
            .chars()
            .take(60)
            .collect()
    }
}
#[derive(Default, Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
pub fn format(response: &Response, command: &Command) -> Result<Output> {
    let mut output = Output::default();
    let mut text = String::new();
    match response {
        Response::Read(result) => {
            output.stdout = read(result, command)?;
        }
        Response::Rendered(bytes) => output.stdout = bytes.clone(),
        Response::Sandbox(path) => writeln!(text, "{}", path.display())?,
        Response::Export { entries, path } => {
            writeln!(text, "Wrote {entries} entries to {}", path.display())?
        }
        Response::Mutation { result, effects } => {
            match result {
                MutationResult::Created {
                    id,
                    chars,
                    has_title,
                } => writeln!(
                    text,
                    "Created entry {id} ({chars} chars{}).",
                    if *has_title { ", title" } else { "" }
                )?,
                MutationResult::Updated { id } => writeln!(text, "Updated entry {id}.")?,
                MutationResult::Deleted { id, hard } => writeln!(
                    text,
                    "Entry {id} {}.",
                    if *hard {
                        "hard-deleted"
                    } else {
                        "marked deleted (Recently Deleted); deletion will sync"
                    }
                )?,
                MutationResult::Restored { id } => {
                    writeln!(text, "Entry {id} restored; the restore will sync.")?
                }
                MutationResult::Emptied { purged, skipped } => {
                    write!(text, "Purged {purged} entries.")?;
                    if *skipped > 0 {
                        write!(
                            text,
                            " Skipped {skipped} synced entries: empty Recently Deleted in Journal.app, or --force to accept resurrection risk."
                        )?;
                    }
                    text.push('\n');
                }
                MutationResult::Repaired { assets, size } => {
                    if *assets == 0 {
                        writeln!(text, "No hidden map assets found. Nothing to repair.")?;
                    } else {
                        writeln!(text, "Repaired {assets} map asset(s) to '{size}'.")?;
                    }
                }
                MutationResult::DryRun {
                    operation,
                    id,
                    repair,
                } => {
                    if let Some((n, size)) = repair {
                        writeln!(
                            text,
                            "DRY RUN: would repair {n} hidden map asset(s) to '{size}'. Nothing written."
                        )?;
                    } else {
                        writeln!(
                            text,
                            "DRY RUN: would {operation}{}. Nothing written.",
                            id.map(|id| format!(" entry {id}")).unwrap_or_else(|| {
                                if *operation == "create" {
                                    " entry".into()
                                } else {
                                    String::new()
                                }
                            })
                        )?;
                    }
                }
            }
            let mut diagnostics = String::new();
            if let Some(path) = &effects.backup {
                writeln!(diagnostics, "backup: {}", path.display())?;
            }
            if effects.staged {
                writeln!(
                    diagnostics,
                    "warning: this is a Mac-local staging membership. Run sync-journals --journal NAME for the native Journal.app steps required to sync that membership."
                )?;
            }
            for warning in &effects.warnings {
                writeln!(diagnostics, "warning: {warning}")?;
            }
            output.stderr = diagnostics.into_bytes();
        }
    }
    if !text.is_empty() {
        output.stdout = text.into_bytes();
    }
    Ok(output)
}
fn read(result: &ReadResult, command: &Command) -> Result<Vec<u8>> {
    let mut out = String::new();
    match result {
        ReadResult::Entries(entries) => {
            let options = match command {
                Command::List(a) => &a.output,
                Command::Search(a) => &a.output,
                _ => anyhow::bail!("entry-list output requires list or search"),
            };
            if options.json {
                return json(entries);
            }
            if entries.is_empty() {
                out += "No entries.\n";
            } else {
                for e in entries {
                    writeln!(
                        out,
                        "{}{:5}  {}  {}",
                        if e.bookmarked {
                            '*'
                        } else if e.synced {
                            ' '
                        } else {
                            '+'
                        },
                        e.id,
                        e.date.as_deref().unwrap_or(""),
                        head(e)
                    )?;
                    if options.full && !e.text.is_empty() {
                        for line in e.text.split('\n') {
                            writeln!(out, "        {line}")?;
                        }
                        out.push('\n');
                    }
                }
                writeln!(
                    out,
                    "\n{} entr{}.",
                    entries.len(),
                    if entries.len() == 1 { "y" } else { "ies" }
                )?;
            }
        }
        ReadResult::Entry(detail) => {
            if matches!(command,Command::Show(a)if a.json) {
                return json(detail);
            }
            let e = &detail.entry;
            writeln!(
                out,
                "id      {}\nuuid    {}\ndate    {}\ntitle   {}\n\n{}",
                e.id,
                e.uuid.as_deref().unwrap_or(""),
                e.date.as_deref().unwrap_or(""),
                e.title,
                e.text
            )?;
            for a in &detail.assets {
                match &a.details {
                    AssetDetails::Map { places } => {
                        for p in places {
                            writeln!(
                                out,
                                "\nlocation  {}, {}  ({}, {})",
                                p.name.as_deref().unwrap_or(""),
                                p.city.as_deref().unwrap_or(""),
                                p.lat,
                                serde_json::to_string(&p.lon)?
                            )?;
                        }
                    }
                    AssetDetails::Audio {
                        duration,
                        transcript,
                    } => writeln!(
                        out,
                        "\naudio (asset {})  {}s  {}",
                        a.id,
                        serde_json::to_string(duration)?,
                        transcript.as_deref().unwrap_or("")
                    )?,
                    AssetDetails::Link { url, link_title } => writeln!(
                        out,
                        "\nlink  {}  {}",
                        link_title.as_deref().unwrap_or(""),
                        url.as_deref().unwrap_or("")
                    )?,
                    AssetDetails::Drawing { drawing_text } => {
                        writeln!(out, "\ndrawing  {}", drawing_text.as_deref().unwrap_or(""))?
                    }
                    AssetDetails::Other { .. } => {}
                }
                for f in &a.files {
                    writeln!(
                        out,
                        "\n{} (asset {})  {}  {}",
                        a.kind,
                        a.id,
                        if f.exists { "ok" } else { "MISSING" },
                        f.path.display()
                    )?;
                }
            }
        }
        ReadResult::Deleted(entries) => {
            if matches!(command,Command::Deleted(a)if a.json) {
                return json(entries);
            }
            if entries.is_empty() {
                out += "Recently Deleted is empty.\n";
            } else {
                for e in entries {
                    writeln!(
                        out,
                        "{:5}  entry {}  deleted {}  {}",
                        e.id,
                        e.date
                            .as_deref()
                            .unwrap_or("?")
                            .chars()
                            .take(10)
                            .collect::<String>(),
                        e.deleted
                            .as_deref()
                            .unwrap_or("?")
                            .chars()
                            .take(16)
                            .collect::<String>(),
                        if e.title.is_empty() {
                            &e.text
                        } else {
                            &e.title
                        }
                    )?;
                }
                writeln!(
                    out,
                    "\n{} entries in Recently Deleted. restore <id> brings one back; they purge ~30 days after deletion.",
                    entries.len()
                )?;
            }
        }
        ReadResult::Journals(journals) => {
            if matches!(command,Command::Journals(a)if a.json) {
                return json(journals);
            }
            for j in journals {
                writeln!(
                    out,
                    "{:3}  {:24} {:4} entries{}",
                    j.journal.pk,
                    j.journal.name,
                    j.entries,
                    if j.journal.is_default {
                        "  (default)"
                    } else {
                        ""
                    }
                )?;
            }
        }
        ReadResult::Stats(s) => {
            if matches!(command,Command::Stats(a)if a.json) {
                return json(s);
            }
            let n = s.words.to_string();
            let formatted = n
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if i > 0 && (n.len() - i).is_multiple_of(3) {
                        format!(",{c}")
                    } else {
                        c.to_string()
                    }
                })
                .collect::<String>();
            writeln!(
                out,
                "entries      {} with text ({} rows total)\nwords        {formatted}\nattachments  {}\nlocations    {}",
                s.entries, s.rows, s.attachments, s.locations
            )?;
            if let (Some(first), Some(last)) = (&s.first, &s.last) {
                writeln!(out, "range        {} .. {}", &first[..10], &last[..10])?;
            }
            writeln!(out, "\nby year")?;
            for (y, n) in &s.by_year {
                writeln!(out, "  {y}  {n:4}  {}", "#".repeat((*n).min(50)))?;
            }
        }
        ReadResult::Doctor(d) => writeln!(
            out,
            "store    {}\n         readable, {:.1} MB\nattach   {}  {}\nJournal  {}\n\nRead {} entries. All good.",
            d.path.display(),
            d.bytes as f64 / 1e6,
            d.attachments.display(),
            if d.attachments_exist { "ok" } else { "missing" },
            if d.journal_running {
                "RUNNING (quit before writing)"
            } else {
                "not running"
            },
            d.entries
        )?,
        ReadResult::Audit(items) => {
            if items.is_empty() {
                out += "No Mac-local custom-journal memberships found.\n";
            } else {
                out += "Mac-local memberships that require finalization in Journal.app:\n";
                for item in items {
                    writeln!(out, "  {}: {} entries", item.name, item.entries)?;
                }
                out += "\nDirect database relationships lack per-entry iCloud merge data.\n1. In Journal.app create a new final journal with a different name.\n2. Open the staging journal, choose Select Entries > Select All.\n3. Choose Move / Choose Journals and select the final journal.\n4. Wait for the staging journal to reach 0 entries and iCloud to finish syncing.\n\nThis command is read-only.\n";
            }
        }
    }
    Ok(out.into_bytes())
}
