use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::TempDir;
const BIN: &str = env!("CARGO_BIN_EXE_journal-rs");
struct Fixture {
    dir: TempDir,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Connection::open(dir.path().join("moments.sqlite")).unwrap();
        db.execute_batch(include_str!("upstream/fixture-schema.sql"))
            .unwrap();
        for (id, name) in [
            (3, "JournalEntryAssetFileAttachmentMO"),
            (4, "JournalEntryAssetMO"),
            (5, "JournalEntryMO"),
            (6, "JournalMO"),
        ] {
            db.execute(
                "insert into Z_PRIMARYKEY(Z_ENT,Z_NAME,Z_SUPER,Z_MAX) values (?,?,0,0)",
                params![id, name],
            )
            .unwrap();
        }
        db.execute(
            "insert into ZJOURNALMO(Z_PK,Z_ENT,ZSORTCATEGORY,ZUSERDELETED) values(1,6,-10,0)",
            [],
        )
        .unwrap();
        fs::create_dir(dir.path().join("Attachments")).unwrap();
        Self { dir }
    }
    fn path(&self) -> std::path::PathBuf {
        self.dir.path().join("moments.sqlite")
    }
    fn db(&self) -> Connection {
        Connection::open(self.path()).unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(BIN)
            .arg("--db")
            .arg(self.path())
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> Output {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&o.stderr)
        );
        o
    }
    fn json(&self, args: &[&str]) -> Value {
        serde_json::from_slice(&self.ok(args).stdout).unwrap()
    }
    fn count(&self, table: &str) -> i64 {
        self.db()
            .query_row(&format!("select count(*) from {table}"), [], |r| r.get(0))
            .unwrap()
    }
}
fn files(path: &Path) -> usize {
    fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| if p.is_dir() { files(&p) } else { 1 })
        .sum()
}
#[test]
fn unicode_dates_and_deleted_lifecycle() {
    let f = Fixture::new();
    f.ok(&[
        "write",
        "--title",
        "旅途 🏕️",
        "--body",
        "台灣 café 👨‍👩‍👦",
        "--date",
        "2024-02-29",
        "--bookmark",
    ]);
    let e = f.json(&["show", "1", "--json"]);
    assert_eq!(e["text"], "台灣 café 👨‍👩‍👦");
    assert_eq!(e["title"], "旅途 🏕️");
    assert_eq!(e["bookmarked"], true);
    assert_eq!(e["chars"], 9); // Swift String.count / extended grapheme clusters.
    assert_eq!(
        f.json(&["search", "CAFÉ", "--json"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.json(&["list", "--since", "2024-03-01", "--json"]),
        json!([])
    );
    f.ok(&["delete", "1"]);
    assert_eq!(f.json(&["list", "--json"]), json!([]));
    assert_eq!(f.json(&["deleted", "--json"])[0]["id"], 1);
    f.ok(&["restore", "1"]);
    assert_eq!(f.json(&["show", "1", "--json"])["text"], e["text"]);
}
#[test]
fn wal_snapshot_reads_committed_uncheckpointed_rows_without_changing_source() {
    let f = Fixture::new();
    let db = f.db();
    db.execute_batch("pragma journal_mode=WAL; pragma wal_autocheckpoint=0; insert into ZJOURNALENTRYMO(Z_PK,ZENTRYDATE) values(17,1);").unwrap();
    let paths = [
        f.path(),
        f.path().with_extension("sqlite-wal"),
        f.path().with_extension("sqlite-shm"),
    ];
    let before = paths
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(f.json(&["list", "--include-empty", "--json"])[0]["id"], 17);
    for (p, b) in paths.iter().zip(before) {
        assert_eq!(fs::read(p).unwrap(), b, "{} changed", p.display());
    }
}
#[test]
fn failed_write_rolls_back_rows_primary_keys_and_new_files() {
    let f = Fixture::new();
    let media = f.dir.path().join("clip.mov");
    fs::write(&media, b"fixture movie").unwrap();
    let out = f.run(&[
        "write",
        "--body",
        "rollback",
        "--media",
        media.to_str().unwrap(),
        "--journal",
        "does not exist",
    ]);
    assert!(!out.status.success());
    for table in [
        "ZJOURNALENTRYMO",
        "ZJOURNALENTRYASSETMO",
        "ZJOURNALENTRYASSETFILEATTACHMENTMO",
    ] {
        assert_eq!(f.count(table), 0);
    }
    let max: i64 = f
        .db()
        .query_row("select sum(Z_MAX) from Z_PRIMARYKEY", [], |r| r.get(0))
        .unwrap();
    assert_eq!(max, 0);
    assert_eq!(files(&f.dir.path().join("Attachments")), 0);
}
#[test]
fn failed_edit_preserves_original_attachment_files() {
    let f = Fixture::new();
    let media = f.dir.path().join("clip.mov");
    fs::write(&media, b"fixture movie").unwrap();
    f.ok(&[
        "write",
        "--body",
        "original",
        "--media",
        media.to_str().unwrap(),
    ]);
    let before = f.json(&["show", "1", "--json"]);
    let out = f.run(&[
        "edit",
        "1",
        "--body",
        "changed",
        "--remove-media",
        "1",
        "999999",
    ]);
    assert!(!out.status.success());
    assert_eq!(f.json(&["show", "1", "--json"]), before);
    assert_eq!(files(&f.dir.path().join("Attachments")), 1);
}
#[test]
fn real_store_guards_cover_symlinks_and_hardlinks() {
    let f = Fixture::new();
    let home = f.dir.path().join("home");
    let real = home.join("Library/Group Containers/group.com.apple.moments/Library/moments.sqlite");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::copy(f.path(), &real).unwrap();
    let symlink = f.dir.path().join("symlink.sqlite");
    std::os::unix::fs::symlink(&real, &symlink).unwrap();
    let hardlink = f.dir.path().join("hardlink.sqlite");
    fs::hard_link(&real, &hardlink).unwrap();
    let before = fs::read(&real).unwrap();
    for p in [&real, &symlink, &hardlink] {
        let o = Command::new(BIN)
            .env("HOME", &home)
            .arg("--db")
            .arg(p)
            .args(["write", "--body", "blocked"])
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&o.stderr).contains("without --live"));
    }
    assert_eq!(fs::read(&real).unwrap(), before);
}
#[test]
fn crdt_and_cloud_delete_guards() {
    let f = Fixture::new();
    f.ok(&["write", "--body", "original"]);
    f.db()
        .execute(
            "update ZJOURNALENTRYMO set ZMERGEABLEATTRIBUTES=X'01',ZISUPLOADEDTOCLOUD=1",
            [],
        )
        .unwrap();
    assert!(!f.run(&["edit", "1", "--body", "bad"]).status.success());
    assert!(!f.run(&["delete", "1", "--hard"]).status.success());
    f.ok(&["edit", "1", "--bookmark"]);
    assert_eq!(f.json(&["show", "1", "--json"])["text"], "original");
    f.ok(&["edit", "1", "--body", "forced", "--force"]);
    assert_eq!(f.json(&["show", "1", "--json"])["text"], "forced");
}
#[test]
fn external_metadata_audio_drawing_and_missing_attachments() {
    let f = Fixture::new();
    f.ok(&["write", "--body", "assets"]);
    let ext = f.dir.path().join(".moments_SUPPORT/_EXTERNAL_DATA");
    fs::create_dir_all(&ext).unwrap();
    fs::write(
        ext.join("ABC"),
        br#"{"duration":2.5,"transcriptSegments":[{"text":"hello"},{"text":"world"}]}"#,
    )
    .unwrap();
    f.db().execute("insert into ZJOURNALENTRYASSETMO(Z_PK,ZENTRY,ZASSETTYPE,ZASSETMETADATA) values(1,1,'audio',?)",[b"\x02ABC\0".as_slice()]).unwrap();
    f.db().execute("insert into ZJOURNALENTRYASSETMO(Z_PK,ZENTRY,ZASSETTYPE,ZASSETMETADATA) values(2,1,'drawing',?)",[b"\x01{\"indexableContent\":\"  handwriting  \"}".as_slice()]).unwrap();
    f.db().execute("insert into ZJOURNALENTRYASSETFILEATTACHMENTMO(Z_PK,ZASSET,ZFILEPATH) values(1,1,'missing.m4a')",[]).unwrap();
    let e = f.json(&["show", "1", "--json"]);
    assert_eq!(e["assets"][0]["transcript"], "hello world");
    assert_eq!(e["assets"][0]["files"][0]["exists"], false);
    assert_eq!(e["assets"][1]["drawing_text"], "handwriting");
}
#[test]
fn dry_run_and_invalid_inputs_do_not_mutate() {
    let f = Fixture::new();
    f.ok(&["write", "--body", "original"]);
    let before = fs::read(f.path()).unwrap();
    for args in [
        vec!["write", "--body", "new", "--dry-run"],
        vec!["edit", "1", "--body", "new", "--dry-run"],
        vec!["delete", "1", "--hard", "--dry-run"],
        vec!["empty", "--dry-run"],
        vec!["repair-locations", "--dry-run"],
    ] {
        f.ok(&args);
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
    for args in [
        vec!["list", "--limit", "-1"],
        vec!["write", "--body", "x", "--lat", "nan", "--lon", "2"],
        vec!["write", "--body", "x", "--date", "2024-02-31"],
        vec!["edit", "1", "--remove-media", "oops"],
        vec!["restore", "99999", "--dry-run"],
    ] {
        assert_eq!(f.run(&args).status.code(), Some(1));
        assert_eq!(fs::read(f.path()).unwrap(), before);
    }
}
#[test]
fn markdown_render_matches_stored_rtf_and_plain_text() {
    let f = Fixture::new();
    let md = "# 回顧\n\n- **coffee**\n- *walk*\n\n`literal **stars**`";
    f.ok(&["write", "--markdown", "--body", md]);
    let rendered = f.ok(&["render", "--body", md]);
    let stored: Vec<u8> = f
        .db()
        .query_row("select ZTEXT from ZJOURNALENTRYMO", [], |r| r.get(0))
        .unwrap();
    assert_eq!(stored, rendered.stdout);
    let plain = String::from_utf8(f.ok(&["render", "--body", md, "--plain"]).stdout).unwrap();
    assert_eq!(f.json(&["show", "1", "--json"])["text"], plain);
}

#[test]
fn relative_database_exports_absolute_attachment_paths() {
    let f = Fixture::new();
    let media = f.dir.path().join("clip.mov");
    fs::write(&media, b"fixture movie").unwrap();
    f.ok(&[
        "write",
        "--body",
        "relative",
        "--media",
        media.to_str().unwrap(),
    ]);
    let run = |args: &[&str]| {
        Command::new(BIN)
            .current_dir(f.dir.path())
            .args(["--db", "moments.sqlite"])
            .args(args)
            .output()
            .unwrap()
    };
    let output = run(&["show", "1", "--json"]);
    assert!(output.status.success());
    let entry: Value = serde_json::from_slice(&output.stdout).unwrap();
    let path = Path::new(entry["assets"][0]["files"][0]["path"].as_str().unwrap());
    assert!(
        path.is_absolute(),
        "attachment must not depend on the export directory: {}",
        path.display()
    );
    assert!(path.is_file());
    let export = run(&["export", "--dir", "elsewhere"]);
    assert!(export.status.success());
    let md = fs::read_to_string(
        fs::read_dir(f.dir.path().join("elsewhere"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path(),
    )
    .unwrap();
    assert!(md.contains(&format!("]({})", path.display())));
}

#[test]
fn edited_media_date_is_now_but_location_visit_keeps_entry_date() {
    let f = Fixture::new();
    f.ok(&["write", "--body", "old", "--date", "2020-01-01"]);
    let media = f.dir.path().join("clip.mov");
    fs::write(&media, b"fixture movie").unwrap();
    let before = chrono::Utc::now().timestamp() as f64 - 978307200.;
    f.ok(&[
        "edit",
        "1",
        "--add-media",
        media.to_str().unwrap(),
        "--lat",
        "25",
        "--lon",
        "121",
    ]);
    let after = chrono::Utc::now().timestamp() as f64 - 978307200. + 1.;
    let db = f.db();
    let blob: Vec<u8> = db
        .query_row(
            "select ZASSETMETADATA from ZJOURNALENTRYASSETMO where ZASSETTYPE='video'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let meta: Value = serde_json::from_slice(&blob[1..]).unwrap();
    let date = meta["date"].as_f64().unwrap();
    assert!(
        (before..=after).contains(&date),
        "edit media timestamp {date} must be near now {before}"
    );
    let entry_date: f64 = db
        .query_row(
            "select ZENTRYDATE from ZJOURNALENTRYMO where Z_PK=1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let blob: Vec<u8> = db
        .query_row(
            "select ZASSETMETADATA from ZJOURNALENTRYASSETMO where ZASSETTYPE='multiPinMap'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let meta: Value = serde_json::from_slice(&blob[1..]).unwrap();
    assert_eq!(meta["visitsData"][0]["visitStartTime"], entry_date);
}

#[test]
fn wrong_sqlite_types_fail_instead_of_disabling_sync_guards() {
    let f = Fixture::new();
    f.ok(&["write", "--body", "protected"]);
    f.db()
        .execute("update ZJOURNALENTRYMO set ZISUPLOADEDTOCLOUD='broken'", [])
        .unwrap();
    assert!(!f.run(&["delete", "1", "--hard"]).status.success());
    assert_eq!(f.count("ZJOURNALENTRYMO"), 1);
    assert!(!f.run(&["show", "1", "--json"]).status.success());
}
#[test]
fn invalid_primary_key_bookkeeping_aborts_without_allocating_rows() {
    let f = Fixture::new();
    f.db()
        .execute(
            "update Z_PRIMARYKEY set Z_MAX='broken' where Z_NAME='JournalEntryMO'",
            [],
        )
        .unwrap();
    let out = f.run(&["write", "--body", "not written"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("primary key bookkeeping"));
    assert_eq!(f.count("ZJOURNALENTRYMO"), 0);
}
#[test]
fn prepared_rtf_round_trips_and_invalid_rtf_does_not_write() {
    let f = Fixture::new();
    let path = f.dir.path().join("body.rtf");
    let rendered = f.ok(&["render", "--body", "**formatted** 台灣"]);
    fs::write(&path, &rendered.stdout).unwrap();
    f.ok(&["write", "--body-rtf", path.to_str().unwrap()]);
    let stored: Vec<u8> = f
        .db()
        .query_row("select ZTEXT from ZJOURNALENTRYMO where Z_PK=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(stored, rendered.stdout);
    fs::write(&path, b"not rtf").unwrap();
    assert!(
        !f.run(&["write", "--body-rtf", path.to_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(f.count("ZJOURNALENTRYMO"), 1);
}
#[test]
fn malformed_metadata_is_reported_with_asset_context() {
    let f = Fixture::new();
    f.ok(&["write", "--body", "metadata"]);
    f.db().execute("insert into ZJOURNALENTRYASSETMO(Z_PK,ZENTRY,ZASSETTYPE,ZASSETMETADATA) values(71,1,'audio',?)",[b"\x01not-json".as_slice()]).unwrap();
    let output = f.run(&["show", "1", "--json"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("asset 71 metadata"));
    f.db()
        .execute(
            "update ZJOURNALENTRYASSETMO set ZASSETMETADATA=? where Z_PK=71",
            [b"\x01{\"duration\":\"wrong type\"}".as_slice()],
        )
        .unwrap();
    let output = f.run(&["show", "1", "--json"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("asset 71 has invalid metadata fields")
    );
}
#[test]
fn library_operations_return_typed_data_before_presentation() {
    use clap::Parser;
    use journal_rs::{
        cli::Cli,
        model::AssetDetails,
        response::{MutationResult, ReadResult, Response},
    };
    let f = Fixture::new();
    let cli = Cli::try_parse_from([
        "journal-rs",
        "write",
        "--body",
        "typed result",
        "--lat",
        "1",
        "--lon",
        "2",
    ])
    .unwrap();
    let response = journal_rs::execute(&f.path(), &cli.command).unwrap();
    assert!(matches!(
        response,
        Response::Mutation {
            result: MutationResult::Created { id: 1, .. },
            ..
        }
    ));
    let cli = Cli::try_parse_from(["journal-rs", "show", "1", "--json"]).unwrap();
    let response = journal_rs::execute(&f.path(), &cli.command).unwrap();
    match &response {
        Response::Read(ReadResult::Entry(detail)) => {
            assert_eq!(detail.entry.text, "typed result");
            assert!(
                matches!(&detail.assets[0].details,AssetDetails::Map{places}if places[0].lat==1.)
            );
        }
        _ => panic!("wrong response"),
    }
    let output = journal_rs::presentation::format(&response, &cli.command).unwrap();
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["assets"][0]["type"], "multiPinMap");
    assert!(output.stderr.is_empty());
}
#[test]
fn sandbox_does_not_reuse_or_overwrite_existing_files() {
    let f = Fixture::new();
    let dest = f.dir.path().join("copy");
    fs::create_dir(&dest).unwrap();
    let stale = dest.join("moments.sqlite-wal");
    fs::write(&stale, b"existing data").unwrap();
    let output = f.run(&[
        "sandbox",
        "--from",
        f.path().to_str().unwrap(),
        "--dir",
        dest.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    assert_eq!(fs::read(&stale).unwrap(), b"existing data");
    assert!(!dest.join("moments.sqlite").exists());
}

#[test]
fn unknown_asset_metadata_does_not_inherit_known_type_constraints() {
    use clap::Parser;
    use journal_rs::{
        cli::Cli,
        model::AssetDetails,
        response::{ReadResult, Response},
    };
    let f = Fixture::new();
    f.ok(&["write", "--body", "future asset"]);
    let metadata = b"\x01{\"duration\":\"future representation\",\"latitude\":25,\"longitude\":121,\"placeName\":\"Taipei\"}";
    f.db().execute("insert into ZJOURNALENTRYASSETMO(Z_PK,ZENTRY,ZASSETTYPE,ZASSETMETADATA) values(71,1,'futureType',?)",[metadata.as_slice()]).unwrap();
    let cli = Cli::try_parse_from(["journal-rs", "show", "1", "--json"]).unwrap();
    let Response::Read(ReadResult::Entry(detail)) =
        journal_rs::execute(&f.path(), &cli.command).unwrap()
    else {
        panic!("wrong response")
    };
    let AssetDetails::Other {
        place: Some(place),
        raw,
    } = &detail.assets[0].details
    else {
        panic!("unknown metadata lost")
    };
    assert_eq!(place.name.as_deref(), Some("Taipei"));
    assert_eq!(raw["duration"], "future representation");
}

#[test]
fn malformed_asset_ordering_rolls_back_body_and_new_attachments() {
    let f = Fixture::new();
    f.ok(&["write", "--body", "original"]);
    let media = f.dir.path().join("video.mov");
    fs::write(&media, b"synthetic movie").unwrap();
    for ordering in [
        "not json",
        "[\"odd pair\"]",
        "[42,0]",
        "[\"uuid\",\"bad index\"]",
        "[\"uuid\",9223372036854775807]",
    ] {
        f.db()
            .execute(
                "update ZJOURNALENTRYMO set ZASSETORDERING=?",
                [ordering.as_bytes()],
            )
            .unwrap();
        let output = f.run(&[
            "edit",
            "1",
            "--body",
            "changed",
            "--add-media",
            media.to_str().unwrap(),
        ]);
        assert!(
            !output.status.success(),
            "accepted invalid ordering {ordering}"
        );
        assert_eq!(f.json(&["show", "1", "--json"])["text"], "original");
        assert_eq!(f.count("ZJOURNALENTRYASSETMO"), 0);
        assert_eq!(files(&f.dir.path().join("Attachments")), 0);
        let stored: Vec<u8> = f
            .db()
            .query_row(
                "select ZASSETORDERING from ZJOURNALENTRYMO where Z_PK=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, ordering.as_bytes());
    }
}

#[test]
fn human_read_commands_preserve_content_and_attachment_diagnostics() {
    let f = Fixture::new();
    let text = |args: &[&str]| String::from_utf8(f.ok(args).stdout).unwrap();
    assert!(text(&["list"]).contains("No entries"));
    assert!(text(&["deleted"]).contains("empty"));
    assert!(text(&["sync-journals"]).contains("No Mac-local"));
    let media = f.dir.path().join("clip.mov");
    fs::write(&media, b"synthetic movie").unwrap();
    f.ok(&[
        "write",
        "--title",
        "旅途",
        "--body",
        "first line\nsecond line",
        "--date",
        "2024-02-29",
        "--bookmark",
        "--lat",
        "25",
        "--lon",
        "121",
        "--place",
        "公園",
        "--city",
        "Taipei",
        "--media",
        media.to_str().unwrap(),
        "--link",
        "https://example.com",
        "--link-title",
        "Website",
    ]);
    f.ok(&["write", "--body", "untitled entry"]);
    let listing = text(&["list", "--full"]);
    for expected in [
        "旅途",
        "first line",
        "second line",
        "untitled entry",
        "2 entries.",
    ] {
        assert!(
            listing.contains(expected),
            "missing {expected} from {listing}"
        );
    }
    assert!(text(&["search", "second", "--full"]).contains("1 entry."));
    for (id, kind, metadata) in [
        (
            71,
            "audio",
            b"\x01{\"duration\":2.5,\"transcriptSegments\":[{\"text\":\"spoken memory\"}]}"
                .as_slice(),
        ),
        (
            72,
            "drawing",
            b"\x01{\"indexableContent\":\"handwritten memory\"}".as_slice(),
        ),
    ] {
        f.db().execute("insert into ZJOURNALENTRYASSETMO(Z_PK,ZENTRY,ZASSETTYPE,ZASSETMETADATA) values(?,1,?,?)", params![id,kind,metadata]).unwrap();
    }
    let detail = f.json(&["show", "1", "--json"]);
    let file = detail["assets"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|a| a["files"][0]["path"].as_str())
        .unwrap();
    assert!(text(&["show", "1"]).contains("  ok  "));
    fs::remove_file(file).unwrap();
    let shown = text(&["show", "1"]);
    for expected in [
        "旅途",
        "second line",
        "公園",
        "Taipei",
        "spoken memory",
        "handwritten memory",
        "https://example.com",
        "Website",
        "MISSING",
    ] {
        assert!(shown.contains(expected), "missing {expected} from {shown}");
    }
    assert!(text(&["journals"]).contains("(default)"));
    assert_eq!(f.json(&["stats", "--json"])["entries"], 2);
    assert!(text(&["doctor"]).contains("Read 2 entries. All good."));
    f.ok(&["delete", "1"]);
    f.ok(&["delete", "2"]);
    let deleted = text(&["deleted"]);
    assert!(deleted.contains("旅途") && deleted.contains("untitled entry"));
}

struct McpClient {
    child: std::process::Child,
    input: Option<std::process::ChildStdin>,
    messages: std::sync::mpsc::Receiver<Value>,
    next_id: u64,
}
impl McpClient {
    fn start(path: &Path) -> Self {
        Self::start_with_home(path, None)
    }
    fn start_with_home(path: &Path, home: Option<&Path>) -> Self {
        use std::io::BufRead;
        let mut command = Command::new(BIN);
        if let Some(home) = home {
            command.env("HOME", home);
        }
        let mut child = command
            .args(["--db", path.to_str().unwrap(), "mcp"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let output = child.stdout.take().unwrap();
        let (send, messages) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(output).lines() {
                let value = serde_json::from_str(&line.unwrap()).unwrap();
                if send.send(value).is_err() {
                    break;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            messages,
            next_id: 0,
        };
        let init = client.request(
            "initialize",
            json!({
                "protocolVersion": "2025-03-26", "capabilities": {},
                "clientInfo": {"name":"journal-tests", "version":"1"}
            }),
        );
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        client.send(json!({"jsonrpc":"2.0", "method":"notifications/initialized"}));
        client
    }
    fn send(&mut self, value: Value) {
        use std::io::Write;
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        self.send(json!({"jsonrpc":"2.0", "id":self.next_id, "method":method, "params":params}));
        loop {
            let message = self
                .messages
                .recv_timeout(std::time::Duration::from_secs(15))
                .unwrap();
            if message["id"] == self.next_id {
                return message;
            }
        }
    }
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":arguments}))
    }
    fn data(&mut self, name: &str, arguments: Value) -> Value {
        let response = self.call(name, arguments);
        assert!(response.get("error").is_none(), "{response}");
        assert_ne!(response["result"]["isError"], true, "{response}");
        response["result"]["structuredContent"].clone()
    }
}
impl Drop for McpClient {
    fn drop(&mut self) {
        self.input.take();
        // Bound cleanup even when a protocol assertion fails.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn mcp_reads_filters_pages_errors_and_never_writes() {
    let f = Fixture::new();
    for (date, title, text) in [
        ("2024-09-10T12:00:00", "Earlier", "Moving felt difficult"),
        ("2025-09-30T23:59:59", "Food", "拉麵 café"),
        ("2025-09-30T23:59:59", "Trip", "拉麵 on holiday"),
        ("2025-10-01T00:00:00", "Boundary", "October"),
        ("2025-09-01", "Deleted", "not visible"),
    ] {
        f.ok(&["write", "--date", date, "--title", title, "--body", text]);
    }
    f.db().execute_batch("update ZJOURNALENTRYMO set ZRECENTLYDELETED=1 where Z_PK=5;
        insert into ZJOURNALMO(Z_PK,Z_ENT,ZUSERDELETED,ZMERGEABLEATTRIBUTES) values(2,6,0,X'637264740054726176656C007469746C6500');
        insert into Z_5JOURNALS(Z_5ENTRIES,Z_6JOURNALS) values(3,2);").unwrap();
    let before = fs::read(f.path()).unwrap();
    let mut mcp = McpClient::start(&f.path());
    let tools = mcp.request("tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 7);
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        let mutation = ["write_entry", "edit_entry", "export_entries"].contains(&name);
        assert_eq!(tool["annotations"]["readOnlyHint"], !mutation);
        assert_eq!(tool["annotations"]["destructiveHint"], name == "edit_entry");
    }
    let first = mcp.data(
        "list_entries",
        json!({"since":"2025-09-01", "until":"2025-10-01", "limit":1}),
    );
    assert_eq!(first["total"], 2);
    assert_eq!(first["entries"][0]["id"], 3);
    assert_eq!(first["entries"][0]["journal_ids"], json!([2]));
    assert_eq!(first["next_offset"], 1);
    let second = mcp.data(
        "list_entries",
        json!({"since":"2025-09-01", "until":"2025-10-01", "limit":1, "offset":1}),
    );
    assert_eq!(second["entries"][0]["id"], 2);
    assert_eq!(second["entries"][0]["journal_ids"], json!([1]));
    assert!(second["next_offset"].is_null());
    assert_eq!(
        mcp.data("search_entries", json!({"query":"拉麵", "journal":"2"}))["total"],
        1
    );
    assert_eq!(
        mcp.data("search_entries", json!({"query":"CAFÉ", "journal":"1"}))["total"],
        1
    );
    assert_eq!(
        mcp.data("search_entries", json!({"query":"absent"}))["total"],
        0
    );
    assert_eq!(
        mcp.data("list_entries", json!({"offset":100}))["entries"],
        json!([])
    );
    let detail = mcp.data("get_entry", json!({"id":3}));
    assert_eq!(detail["text"], "拉麵 on holiday");
    assert_eq!(detail["journal_ids"], json!([2]));
    assert!(detail["assets"].is_array());
    assert_eq!(
        mcp.data("list_journals", json!({}))["journals"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    for (tool, args) in [
        ("get_entry", json!({"id":5})),
        ("get_entry", json!({"id":999})),
        ("list_entries", json!({"limit":0})),
        ("list_entries", json!({"limit":101})),
        ("list_entries", json!({"since":"bad"})),
        (
            "list_entries",
            json!({"since":"2026-01-01", "until":"2025-01-01"}),
        ),
        ("list_entries", json!({"journal":"missing"})),
        ("search_entries", json!({"query":" "})),
        ("search_entries", json!({})),
    ] {
        assert_eq!(mcp.call(tool, args)["result"]["isError"], true);
    }
    let bad = mcp.call("get_entry", json!({"id":3,"db":"/tmp/another.sqlite"}));
    assert!(bad.get("error").is_some() || bad["result"]["isError"] == true);
    assert!(
        mcp.call("write", json!({"body":"forbidden"}))
            .get("error")
            .is_some()
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);

    // The CLI shares filters but retains its existing inclusive upper date boundary.
    assert_eq!(
        f.json(&[
            "list",
            "--since",
            "2025-09-01",
            "--until",
            "2025-10-01",
            "--json"
        ])
        .as_array()
        .unwrap()
        .len(),
        3
    );
    assert_eq!(
        f.json(&[
            "search",
            "拉麵",
            "--journal",
            "2",
            "--since",
            "2025-09-01",
            "--until",
            "2025-10-01",
            "--json"
        ])[0]["id"],
        3
    );
    assert_eq!(
        f.json(&["list", "--journal", "1", "--json"])
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn mcp_missing_database_returns_tool_error_and_stays_alive() {
    let dir = tempfile::tempdir().unwrap();
    let mut mcp = McpClient::start(&dir.path().join("missing.sqlite"));
    assert_eq!(
        mcp.call("list_entries", json!({}))["result"]["isError"],
        true
    );
    assert!(mcp.request("tools/list", json!({}))["result"]["tools"].is_array());
    assert!(!dir.path().join("missing.sqlite").exists());
}

#[test]
fn mcp_text_mutations_preview_preserve_fields_and_enforce_guards() {
    let f = Fixture::new();
    let original = fs::read(f.path()).unwrap();
    let mut mcp = McpClient::start(&f.path());
    let preview = mcp.data(
        "write_entry",
        json!({"title":"週末", "body":"**散步**", "markdown":true}),
    );
    assert_eq!(preview["result"]["status"], "dry_run");
    assert_eq!(fs::read(f.path()).unwrap(), original);
    let created = mcp.data("write_entry", json!({"title":"週末", "body":"**散步**", "markdown":true, "date":"2025-09-18", "dry_run":false}));
    assert_eq!(created["result"]["status"], "created");
    let id = created["result"]["id"].as_i64().unwrap();
    assert_eq!(mcp.data("get_entry", json!({"id":id}))["text"], "散步");
    let before_edit = fs::read(f.path()).unwrap();
    assert_eq!(
        mcp.data("edit_entry", json!({"id":id,"title":"週末回憶"}))["result"]["status"],
        "dry_run"
    );
    assert_eq!(fs::read(f.path()).unwrap(), before_edit);
    mcp.data(
        "edit_entry",
        json!({"id":id,"title":"週末回憶","bookmark":true,"dry_run":false}),
    );
    let entry = mcp.data("get_entry", json!({"id":id}));
    assert_eq!(entry["title"], "週末回憶");
    assert_eq!(entry["text"], "散步"); // Omitted body must not consume protocol stdin or clear text.
    assert_eq!(entry["bookmarked"], true);
    mcp.data(
        "edit_entry",
        json!({"id":id,"body":"","bookmark":false,"dry_run":false}),
    );
    let entry = mcp.data("get_entry", json!({"id":id}));
    assert_eq!(entry["text"], "");
    assert_eq!(entry["bookmarked"], false);
    assert_eq!(entry["title"], "週末回憶");
    let title_only = mcp.data("write_entry", json!({"title":"只有標題","dry_run":false}));
    assert_eq!(title_only["result"]["status"], "created");
    for (name, args) in [
        ("write_entry", json!({})),
        ("write_entry", json!({"body":"x","date":"bad"})),
        (
            "write_entry",
            json!({"body":"x","journal":"missing","dry_run":false}),
        ),
        ("edit_entry", json!({"id":999,"title":"x"})),
        ("edit_entry", json!({"id":id})),
    ] {
        assert_eq!(mcp.call(name, args)["result"]["isError"], true);
    }
    f.db()
        .execute(
            "update ZJOURNALENTRYMO set ZMERGEABLEATTRIBUTES=X'01' where Z_PK=?",
            [id],
        )
        .unwrap();
    assert_eq!(
        mcp.call(
            "edit_entry",
            json!({"id":id,"body":"no bypass","dry_run":false})
        )["result"]["isError"],
        true
    );
    mcp.data(
        "edit_entry",
        json!({"id":id,"bookmark":true,"dry_run":false}),
    );
    f.db()
        .execute(
            "update ZJOURNALENTRYMO set ZRECENTLYDELETED=1 where Z_PK=?",
            [id],
        )
        .unwrap();
    assert_eq!(
        mcp.call(
            "edit_entry",
            json!({"id":id,"bookmark":false,"dry_run":false})
        )["result"]["isError"],
        true
    );
}

#[test]
fn mcp_exports_preview_and_refuse_overwrite() {
    let f = Fixture::new();
    f.ok(&["write", "--title", "旅程", "--body", "日記內容"]);
    let before = fs::read(f.path()).unwrap();
    let mut mcp = McpClient::start(&f.path());
    for format in ["json", "md"] {
        let dir = f.dir.path().join(format);
        let preview = mcp.data("export_entries", json!({"dir":dir,"format":format}));
        assert_eq!(preview["dry_run"], true);
        assert_eq!(preview["entries"], 1);
        assert!(!dir.exists());
        let done = mcp.data(
            "export_entries",
            json!({"dir":dir,"format":format,"dry_run":false}),
        );
        assert_eq!(done["dry_run"], false);
        let file = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
        let content = fs::read_to_string(&file).unwrap();
        assert!(content.contains("日記內容"));
        assert_eq!(
            mcp.call(
                "export_entries",
                json!({"dir":dir,"format":format,"dry_run":false})
            )["result"]["isError"],
            true
        );
        assert_eq!(fs::read_to_string(&file).unwrap(), content);
    }
    let link = f.dir.path().join("dangling");
    std::os::unix::fs::symlink(f.dir.path().join("absent"), &link).unwrap();
    assert_eq!(
        mcp.call(
            "export_entries",
            json!({"dir":link,"format":"json","dry_run":false})
        )["result"]["isError"],
        true
    );
    assert_eq!(
        mcp.call("export_entries", json!({"dir":"relative","format":"json"}))["result"]["isError"],
        true
    );
    assert_eq!(fs::read(f.path()).unwrap(), before);
}

#[test]
fn mcp_live_write_requires_explicit_opt_in_even_through_an_alias() {
    let f = Fixture::new();
    let home = f.dir.path().join("home");
    let real = home.join("Library/Group Containers/group.com.apple.moments/Library/moments.sqlite");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::copy(f.path(), &real).unwrap();
    let alias = f.dir.path().join("alias.sqlite");
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    let before = fs::read(&real).unwrap();
    let mut mcp = McpClient::start_with_home(&alias, Some(&home));
    mcp.data("write_entry", json!({"body":"preview"}));
    let response = mcp.call("write_entry", json!({"body":"must refuse","dry_run":false}));
    assert_eq!(response["result"]["isError"], true);
    assert!(
        response["result"]["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("without --live")
    );
    // With live opted in but no risk acceptance, either the running-app or risk guard refuses.
    assert_eq!(
        mcp.call(
            "write_entry",
            json!({"body":"must refuse","dry_run":false,"live":true})
        )["result"]["isError"],
        true
    );
    assert_eq!(fs::read(&real).unwrap(), before);
    assert!(!home.join(".config/journal-rs/risk-accepted").exists());
}

#[test]
fn cli_equal_date_order_is_preserved_while_mcp_pages_are_deterministic() {
    let f = Fixture::new();
    for body in ["match first", "match second", "match third"] {
        f.ok(&["write", "--body", body, "--date", "2025-09-18"]);
    }
    for args in [vec!["list", "--json"], vec!["search", "match", "--json"]] {
        let rows = f.json(&args);
        let ids: Vec<_> = rows
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids, vec![1, 2, 3]);
        let mut limited = args;
        limited.extend(["--limit", "1"]);
        assert_eq!(f.json(&limited)[0]["id"], 1);
    }
    let mut mcp = McpClient::start(&f.path());
    for offset in 0..3 {
        let page = mcp.data("list_entries", json!({"limit":1,"offset":offset}));
        assert_eq!(page["entries"][0]["id"], 3 - offset);
    }
}

#[test]
fn active_edit_rechecks_state_after_an_earlier_read() {
    use clap::Parser;
    use journal_rs::cli::{Cli, Command as Request};
    let f = Fixture::new();
    f.ok(&["write", "--body", "original"]);
    assert_eq!(f.json(&["list", "--json"])[0]["id"], 1);
    // Another connection commits a deletion after the caller has read the entry.
    // Exercise the transaction guard directly, without MCP's snapshot preflight.
    for update in [
        "ZRECENTLYDELETED=1",
        "ZRECENTLYDELETED=0,ZISFULLYREMOVED=1",
        "ZISFULLYREMOVED=0,ZENTRYDATE=NULL",
    ] {
        f.db()
            .execute(
                &format!("update ZJOURNALENTRYMO set {update} where Z_PK=1"),
                [],
            )
            .unwrap();
        let mut cli =
            Cli::try_parse_from(["journal-rs", "edit", "1", "--body", "must not be written"])
                .unwrap();
        if let Request::Edit(ref mut edit) = cli.command {
            edit.require_active = true;
        }
        let before = fs::read(f.path()).unwrap();
        let error = journal_rs::execute(&f.path(), &cli.command).unwrap_err();
        assert!(error.to_string().contains("no active entry"), "{error:#}");
        assert_eq!(fs::read(f.path()).unwrap(), before);
        assert_eq!(f.json(&["show", "1", "--json"])["text"], "original");
    }
}
