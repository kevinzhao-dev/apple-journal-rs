//! Shared selection semantics for CLI reads and MCP tools.
use crate::model::{self, Entry};
use anyhow::{Result, ensure};
use rusqlite::Connection;
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct Filter<'a> {
    pub since: Option<f64>,
    pub until: Option<f64>,
    pub exclusive_until: bool,
    pub journal: Option<&'a str>,
    pub text: Option<&'a str>,
    pub include_empty: bool,
}

/// Includes implicit default-journal membership, matching the journals command.
pub(crate) fn memberships(db: &Connection) -> Result<BTreeMap<i64, Vec<i64>>> {
    let mut result = BTreeMap::<i64, Vec<i64>>::new();
    for (entry, journal) in model::query(
        db,
        "select distinct Z_5ENTRIES,Z_6JOURNALS from Z_5JOURNALS order by Z_5ENTRIES,Z_6JOURNALS",
        [],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
    )? {
        result.entry(entry).or_default().push(journal);
    }
    let defaults: Vec<_> = model::journals(db)?
        .into_iter()
        .filter(|j| j.is_default)
        .map(|j| j.pk)
        .collect();
    if !defaults.is_empty() {
        for id in model::ids(db, "select Z_PK from ZJOURNALENTRYMO", [])? {
            result.entry(id).or_insert_with(|| defaults.clone());
        }
    }
    Ok(result)
}

pub(crate) fn entries(db: &Connection, filter: Filter<'_>) -> Result<Vec<Entry>> {
    if let (Some(since), Some(until)) = (filter.since, filter.until) {
        ensure!(since <= until, "since must not be after until");
    }
    let membership = if let Some(journal) = filter.journal {
        Some((model::resolve_journal(db, journal)?.pk, memberships(db)?))
    } else {
        None
    };
    let text = filter.text.map(str::to_lowercase);
    let entries: Vec<_> = model::fetch(db, filter.include_empty)?
        .into_iter()
        .filter(|entry| {
            let timestamp = entry.timestamp.expect("fetch returns dated entries");
            filter.since.is_none_or(|since| timestamp >= since)
                && filter.until.is_none_or(|until| {
                    if filter.exclusive_until {
                        timestamp < until
                    } else {
                        timestamp <= until
                    }
                })
                && membership
                    .as_ref()
                    .is_none_or(|(id, map)| map.get(&entry.id).is_some_and(|ids| ids.contains(id)))
                && text.as_ref().is_none_or(|query| {
                    entry.title.to_lowercase().contains(query)
                        || entry.text.to_lowercase().contains(query)
                })
        })
        .collect();
    Ok(entries)
}
