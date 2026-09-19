//! Mutation orchestration. Prepares inputs, executes one transaction, returns results.
use crate::{
    cli::{Command, MutationOptions},
    codec,
    input::{self, PreparedEntry},
    model,
    mutation::Mutation,
    response::{MutationEffects, MutationResult, Response},
    store,
};
use anyhow::Result;
use std::path::Path;

fn transact(
    path: &Path,
    options: &MutationOptions,
    apply: impl FnOnce(&mut Mutation<'_>) -> Result<MutationResult>,
) -> Result<Response> {
    let mut writable = store::writable(path, options)?;
    let tx = writable
        .db
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let mut work = Mutation::new(&tx, path, writable.backup);
    let result = apply(&mut work)?;
    let integrity: String = tx.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    anyhow::ensure!(integrity == "ok", "integrity check failed: {integrity}");
    // Drop the connection borrow before consuming the transaction. Files stay armed until commit.
    let (files, mut effects) = work.into_effects();
    tx.commit()?;
    effects.warnings.extend(files.finish());
    Ok(Response::Mutation { result, effects })
}
fn dry_run(
    path: &Path,
    operation: &'static str,
    id: Option<i64>,
    repair: Option<&'static str>,
) -> Result<Response> {
    let repair = if id.is_some() || repair.is_some() {
        let s = store::Snapshot::new(path)?;
        if let Some(id) = id {
            model::fetch_one(&s.db, id)?;
        }
        if let Some(size) = repair {
            let count = s.db.query_row(
                "select count(*) from ZJOURNALENTRYASSETMO where ZASSETTYPE='multiPinMap' and coalesce(ZISHIDDEN,0)=1",
                [],
                |r| r.get(0),
            )?;
            Some((count, size))
        } else {
            None
        }
    } else {
        None
    };
    Ok(Response::Mutation {
        result: MutationResult::DryRun {
            operation,
            id,
            repair,
        },
        effects: MutationEffects::default(),
    })
}
pub fn run(path: &Path, command: &Command) -> Result<Response> {
    match command {
        Command::Render(a) => {
            let text =
                input::read_body(a.body.as_deref(), a.body_file.as_deref())?.unwrap_or_default();
            let data = if a.inline {
                codec::inline(&text)?.into_bytes()
            } else {
                let (rtf, plain) = codec::encode(&text, true)?;
                if a.plain { plain.into_bytes() } else { rtf }
            };
            Ok(Response::Rendered(data))
        }
        Command::Write(a) => {
            let entry = PreparedEntry::new(
                &a.entry,
                &a.media,
                &a.live_photo,
                a.link.as_deref(),
                &a.media_options,
            )?;
            anyhow::ensure!(
                entry.has_content(),
                "nothing to write (need title, body, media, link or location)"
            );
            if a.safety.dry_run {
                return dry_run(path, "create", None, None);
            }
            transact(path, &a.safety, |m| m.create(&entry, a))
        }
        Command::Edit(a) => {
            let entry = PreparedEntry::new(
                &a.entry,
                &a.add_media,
                &[],
                a.add_link.as_deref(),
                &a.media_options,
            )?;
            anyhow::ensure!(
                entry.has_changes()
                    || a.bookmark
                    || a.no_bookmark
                    || a.clear_location
                    || a.remove_all_media
                    || !a.remove_media.is_empty(),
                "nothing to change"
            );
            if a.safety.dry_run {
                return dry_run(path, "edit", Some(a.id), None);
            }
            transact(path, &a.safety, |m| m.edit(&entry, a))
        }
        Command::Delete(a) => {
            if a.safety.dry_run {
                return dry_run(
                    path,
                    if a.hard { "hard-delete" } else { "soft-delete" },
                    Some(a.id),
                    None,
                );
            }
            transact(path, &a.safety, |m| m.delete(a))
        }
        Command::Restore(a) => {
            if a.safety.dry_run {
                return dry_run(path, "restore", Some(a.id), None);
            }
            transact(path, &a.safety, |m| m.restore(a.id))
        }
        Command::Empty(a) => {
            if a.safety.dry_run {
                return dry_run(path, "empty", None, None);
            }
            transact(path, &a.safety, |m| m.empty(a.force))
        }
        Command::RepairLocations(a) => {
            if a.safety.dry_run {
                return dry_run(path, "repair", None, Some(a.to.name()));
            }
            transact(path, &a.safety, |m| m.repair(a.to))
        }
        _ => anyhow::bail!("not a mutation operation"),
    }
}
