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
