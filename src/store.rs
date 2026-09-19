use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tempfile::TempDir;
pub const EPOCH: f64 = 978307200.;
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}
pub fn default_db() -> PathBuf {
    home().join("Library/Group Containers/group.com.apple.moments/Library/moments.sqlite")
}
pub fn now() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000. - EPOCH
}
pub fn date(n: Option<f64>) -> Option<String> {
    n.and_then(|n| Local.timestamp_opt((n + EPOCH).floor() as i64, 0).single())
        .map(|d| d.format("%Y-%m-%d %H:%M:%S").to_string())
}
pub fn parse_date(s: &str) -> Result<f64> {
    let d = NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .or_else(|| {
            ["%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M"]
                .iter()
                .find_map(|f| NaiveDateTime::parse_from_str(s, f).ok())
        })
        .with_context(|| format!("cannot parse date '{s}' (use YYYY-MM-DD)"))?;
    Ok(Local
        .from_local_datetime(&d)
        .earliest()
        .context("date falls in a local clock gap")?
        .timestamp() as f64
        - EPOCH)
}
pub fn uid() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}
pub fn uuid(blob: Option<&[u8]>) -> Option<String> {
    blob.and_then(|b| uuid::Uuid::from_slice(b).ok())
        .map(|u| u.to_string().to_uppercase())
}
pub fn uuid_bytes(s: &str) -> Result<Vec<u8>> {
    Ok(uuid::Uuid::parse_str(s)?.as_bytes().to_vec())
}
pub fn attachments(db: &Path) -> PathBuf {
    db.parent().unwrap_or(Path::new(".")).join("Attachments")
}
pub fn running() -> Result<bool> {
    Ok(std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "Journal"])
        .stdout(std::process::Stdio::null())
        .status()?
        .success())
}
// Copy the store before opening SQLite, preserving the upstream no-live-lock read contract.
pub struct Snapshot {
    pub db: Connection,
    _dir: TempDir,
}
impl Snapshot {
    pub fn new(path: &Path) -> Result<Self> {
        anyhow::ensure!(path.is_file(), "store not found at {}", path.display());
        let dir = tempfile::tempdir()?;
        copy_store(path, &dir.path().join("moments.sqlite"))?;
        let db = Connection::open(dir.path().join("moments.sqlite"))?;
        db.pragma_update(None, "query_only", true)?;
        Ok(Self { db, _dir: dir })
    }
}
pub fn copy_store(src: &Path, dst: &Path) -> Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let from = PathBuf::from(format!("{}{suffix}", src.display()));
        if from.exists() {
            fs::copy(&from, format!("{}{suffix}", dst.display())).with_context(|| {
                format!(
                    "cannot copy {}; grant Full Disk Access if reading Journal",
                    from.display()
                )
            })?;
        }
    }
    Ok(())
}
pub fn sandbox(a: &crate::cli::SandboxRequest) -> Result<PathBuf> {
    let dest = &a.dir;
    let src = a.from.clone().unwrap_or_else(default_db);
    anyhow::ensure!(src.is_file(), "source store not found: {}", src.display());
    fs::create_dir_all(dest)?;
    let dst = dest.join("moments.sqlite");
    for suffix in ["", "-wal", "-shm"] {
        anyhow::ensure!(
            !PathBuf::from(format!("{}{suffix}", dst.display())).exists(),
            "sandbox target already exists: {} (choose a new directory)",
            dst.display()
        );
    }
    copy_store(&src, &dst)?;
    fs::create_dir_all(dest.join("Attachments"))?;
    Ok(dst)
}
pub struct Writable {
    pub db: Connection,
    pub backup: Option<PathBuf>,
}
pub fn writable(db_path: &Path, options: &crate::cli::MutationOptions) -> Result<Writable> {
    let mut backup_path = None;
    let path = fs::canonicalize(db_path)
        .with_context(|| format!("store not found at {}", db_path.display()))?;
    let live = fs::canonicalize(default_db()).ok();
    // Canonical paths cover symlinks; device/inode identity also covers hard links.
    use std::os::unix::fs::MetadataExt;
    let same_inode = live
        .as_ref()
        .and_then(|p| fs::metadata(p).ok())
        .is_some_and(|m| {
            fs::metadata(&path).is_ok_and(|n| n.dev() == m.dev() && n.ino() == m.ino())
        });
    if live.as_ref() == Some(&path) || same_inode {
        anyhow::ensure!(
            options.live,
            "refusing to modify the real Journal store without --live. Test against a sandbox first"
        );
        anyhow::ensure!(
            !running()?,
            "Journal.app is running. Quit it first, then retry."
        );
        let marker = home().join(".config/journal-rs/risk-accepted");
        if !marker.exists() {
            if !options.accept_risk {
                bail!(
                    "first live write requires --accept-risk; export a backup in Journal.app first. Direct writes to the private store can corrupt entries or iCloud sync on all devices. Agents must obtain user authorization first"
                )
            }
            fs::create_dir_all(marker.parent().unwrap())?;
            fs::write(&marker, "accepted\n")?;
        }
        let backup = home().join("Backups/journal-rs").join(format!(
            "{}-{}",
            Local::now().format("%Y%m%d-%H%M%S"),
            uid()
        ));
        fs::create_dir_all(&backup)?;
        copy_store(&path, &backup.join("moments.sqlite"))?;
        backup_path = Some(backup);
    }
    let db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    db.busy_timeout(Duration::from_secs(5))?;
    Ok(Writable {
        db,
        backup: backup_path,
    })
}
