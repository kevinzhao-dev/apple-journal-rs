//! Results returned by operations. Presentation formats these values; main owns terminal I/O.
use crate::model::{Entry, EntryDetail, Journal};
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Debug, Serialize)]
pub struct DeletedEntry {
    pub id: i64,
    pub date: Option<String>,
    pub deleted: Option<String>,
    pub title: String,
    pub text: String,
    pub synced: bool,
}
#[derive(Debug, Serialize)]
pub struct JournalCount {
    #[serde(flatten)]
    pub journal: Journal,
    pub entries: i64,
}
#[derive(Debug, Serialize)]
pub struct Stats {
    pub entries: usize,
    pub rows: usize,
    pub words: usize,
    pub attachments: i64,
    pub locations: i64,
    pub by_year: BTreeMap<String, usize>,
    pub first: Option<String>,
    pub last: Option<String>,
}
#[derive(Debug)]
pub struct Doctor {
    pub path: PathBuf,
    pub bytes: u64,
    pub attachments: PathBuf,
    pub attachments_exist: bool,
    pub journal_running: bool,
    pub entries: usize,
}
#[derive(Debug)]
pub struct AuditItem {
    pub name: String,
    pub entries: i64,
}
#[derive(Debug)]
pub enum ReadResult {
    Entries(Vec<Entry>),
    Entry(EntryDetail),
    Deleted(Vec<DeletedEntry>),
    Journals(Vec<JournalCount>),
    Stats(Stats),
    Doctor(Doctor),
    Audit(Vec<AuditItem>),
}
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum MutationResult {
    Created {
        id: i64,
        chars: usize,
        has_title: bool,
    },
    Updated {
        id: i64,
    },
    Deleted {
        id: i64,
        hard: bool,
    },
    Restored {
        id: i64,
    },
    Emptied {
        purged: usize,
        skipped: usize,
    },
    Repaired {
        assets: usize,
        size: &'static str,
    },
    DryRun {
        operation: &'static str,
        id: Option<i64>,
        repair: Option<(i64, &'static str)>,
    },
}
#[derive(Debug, Default, Serialize)]
pub struct MutationEffects {
    pub staged: bool,
    pub backup: Option<PathBuf>,
    pub warnings: Vec<String>,
}
#[derive(Debug)]
pub enum Response {
    Read(ReadResult),
    Mutation {
        result: MutationResult,
        effects: MutationEffects,
    },
    Export {
        entries: usize,
        path: PathBuf,
    },
    Sandbox(PathBuf),
    Rendered(Vec<u8>),
}
