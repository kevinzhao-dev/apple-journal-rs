use crate::{
    cli::Command,
    model::{self, query},
    response::*,
    store,
};
use anyhow::Result;
use std::{collections::BTreeMap, fs, path::Path};

pub fn run(path: &Path, command: &Command) -> Result<ReadResult> {
    let snapshot = store::Snapshot::new(path)?;
    let db = &snapshot.db;
    Ok(match command {
        Command::List(a) => {
            let mut entries = crate::query::entries(
                db,
                crate::query::Filter {
                    since: a.since,
                    until: a.until,
                    journal: a.journal.as_deref(),
                    include_empty: a.include_empty,
                    ..Default::default()
                },
            )?;
            if let Some(n) = a.output.limit {
                entries.truncate(n);
            }
            ReadResult::Entries(entries)
        }
        Command::Search(a) => {
            let mut entries = crate::query::entries(
                db,
                crate::query::Filter {
                    since: a.since,
                    until: a.until,
                    journal: a.journal.as_deref(),
                    text: Some(&a.query),
                    ..Default::default()
                },
            )?;
            if let Some(n) = a.output.limit {
                entries.truncate(n);
            }
            ReadResult::Entries(entries)
        }
        Command::Show(a) => ReadResult::Entry(model::detail(db, path, a.id)?),
        Command::Stats(_) => {
            let all = model::fetch(db, true)?;
            let real = all
                .iter()
                .filter(|e| !e.text.is_empty())
                .collect::<Vec<_>>();
            let attachments = db.query_row(
                "select count(*) from ZJOURNALENTRYASSETFILEATTACHMENTMO",
                [],
                |r| r.get(0),
            )?;
            let locations=db.query_row("select count(*) from ZJOURNALENTRYASSETMO where ZASSETTYPE in ('multiPinMap','genericMap')",[],|r|r.get(0))?;
            let mut by_year = BTreeMap::<String, usize>::new();
            for e in &real {
                *by_year
                    .entry(e.date.as_ref().unwrap()[..4].into())
                    .or_default() += 1;
            }
            ReadResult::Stats(Stats {
                entries: real.len(),
                rows: all.len(),
                words: real.iter().map(|e| e.text.split_whitespace().count()).sum(),
                attachments,
                locations,
                by_year,
                first: real.last().and_then(|e| e.date.clone()),
                last: real.first().and_then(|e| e.date.clone()),
            })
        }
        Command::Journals(_) => {
            let live = "coalesce(e.ZISFULLYREMOVED,0)=0 and coalesce(e.ZRECENTLYDELETED,0)=0 and coalesce(e.ZISTIP,0)=0 and e.ZENTRYDATE is not null";
            let unassigned:i64=db.query_row(&format!("select count(*) from ZJOURNALENTRYMO e where {live} and not exists(select 1 from Z_5JOURNALS j where j.Z_5ENTRIES=e.Z_PK)"),[],|r|r.get(0))?;
            let mut out = vec![];
            for journal in model::journals(db)? {
                let n:i64=db.query_row(&format!("select count(*) from Z_5JOURNALS j join ZJOURNALENTRYMO e on e.Z_PK=j.Z_5ENTRIES where {live} and j.Z_6JOURNALS=?"),[journal.pk],|r|r.get(0))?;
                let entries = n + if journal.is_default { unassigned } else { 0 };
                out.push(JournalCount { journal, entries });
            }
            ReadResult::Journals(out)
        }
        Command::Deleted(_) => {
            struct DeletedRow {
                id: i64,
                date: Option<f64>,
                deleted: Option<f64>,
                title: Option<Vec<u8>>,
                text: Option<Vec<u8>>,
                synced: bool,
            }
            let rows = query(
                db,
                "select Z_PK,ZENTRYDATE,ZRECENTLYDELETEDENTRYDATE,ZTITLE,ZTEXT,ZISUPLOADEDTOCLOUD from ZJOURNALENTRYMO where coalesce(ZRECENTLYDELETED,0)=1 and coalesce(ZISFULLYREMOVED,0)=0 order by ZRECENTLYDELETEDENTRYDATE desc",
                [],
                |r| {
                    Ok(DeletedRow {
                        id: r.get(0)?,
                        date: r.get(1)?,
                        deleted: r.get(2)?,
                        title: r.get(3)?,
                        text: r.get(4)?,
                        synced: model::flag(r, "ZISUPLOADEDTOCLOUD")?,
                    })
                },
            )?;
            ReadResult::Deleted(
                rows.into_iter()
                    .map(|r| {
                        Ok(DeletedEntry {
                            id: r.id,
                            date: store::date(r.date),
                            deleted: store::date(r.deleted),
                            title: crate::codec::decode(r.title.as_deref())?,
                            text: crate::codec::decode(r.text.as_deref())?,
                            synced: r.synced,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            )
        }
        Command::Doctor => {
            let attachments = store::attachments(path);
            ReadResult::Doctor(Doctor {
                path: path.into(),
                bytes: fs::metadata(path)?.len(),
                attachments_exist: attachments.is_dir(),
                attachments,
                journal_running: store::running()?,
                entries: model::fetch(db, false)?.len(),
            })
        }
        Command::SyncJournals(a) => {
            let journals = if let Some(name) = &a.journal {
                let j = model::resolve_journal(db, name)?;
                anyhow::ensure!(
                    !j.is_default,
                    "the default journal has no custom membership to audit"
                );
                vec![j]
            } else {
                model::journals(db)?
                    .into_iter()
                    .filter(|j| !j.is_default)
                    .collect()
            };
            let mut out = vec![];
            for j in journals {
                let entries:i64=db.query_row("select count(*) from Z_5JOURNALS j join ZJOURNALENTRYMO e on e.Z_PK=j.Z_5ENTRIES where j.Z_6JOURNALS=? and e.ZMERGEABLEATTRIBUTES is null and coalesce(e.ZISFULLYREMOVED,0)=0 and coalesce(e.ZRECENTLYDELETED,0)=0",[j.pk],|r|r.get(0))?;
                if entries > 0 {
                    out.push(AuditItem {
                        name: j.name,
                        entries,
                    });
                }
            }
            ReadResult::Audit(out)
        }
        _ => anyhow::bail!("not a read operation"),
    })
}
