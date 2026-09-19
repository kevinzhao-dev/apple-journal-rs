#!/usr/bin/env python3
"""Compare Rust and a pinned upstream Swift binary on synthetic data only."""
import argparse
import json
import pathlib
import sqlite3
import subprocess
import tempfile

p = argparse.ArgumentParser()
p.add_argument("reference", type=pathlib.Path)
p.add_argument("--rust", type=pathlib.Path, default=pathlib.Path("target/debug/journal-rs"))
a = p.parse_args()
reference, rust = str(a.reference.resolve()), str(a.rust.resolve())
root = pathlib.Path(__file__).resolve().parents[1]
checks = 0

def run(binary, *args):
    return subprocess.check_output([binary, *map(str, args)], stdin=subprocess.DEVNULL)

with tempfile.TemporaryDirectory(prefix="journal-rs-parity-") as tmp:
    d = pathlib.Path(tmp)
    db = d / "moments.sqlite"
    con = sqlite3.connect(db)
    con.executescript((root / "tests/upstream/fixture-schema.sql").read_text())
    con.close()
    subprocess.run(["python3", str(root / "tests/upstream/seed-fixture.py"), str(db)], check=True)
    (d / "Attachments").mkdir()
    movie = d / "video.mov"
    movie.write_bytes(b"synthetic movie")
    # Exercise data created by both implementations, including Unicode and rich text.
    for binary, title in [(reference, "Swift 原始"), (rust, "Rust 移植")]:
        run(binary, "--db", db, "write", "--title", title, "--body", "Coffee café 台灣 👨‍👩‍👦", "--date", "2024-02-29", "--bookmark", "--lat", "25.03", "--lon", "121.56", "--place", 'Cafe "quoted"', "--city", "Taipei", "--media", movie)
        run(binary, "--db", db, "write", "--body", "# Day\n\n- **hello**\n- *world*", "--markdown", "--date", "2025-01-02", "--journal", "Test Journal", "--link", "https://example.com", "--link-title", "Example")
    run(rust, "--db", db, "write", "--title", "Title only", "--date", "2022-01-01")
    run(rust, "--db", db, "write", "--body", "Recently deleted", "--date", "2023-01-01")
    run(rust, "--db", db, "delete", "6")
    with sqlite3.connect(db) as con:
        con.execute("insert into ZJOURNALENTRYMO(Z_PK,ZENTRYDATE) values(99,1)")
        con.execute("insert into ZJOURNALENTRYMO(Z_PK) values(100)")
        ext = d / ".moments_SUPPORT/_EXTERNAL_DATA"
        ext.mkdir(parents=True)
        (ext / "AUDIO").write_text(json.dumps({"duration": 2.5, "transcriptSegments": [{"text": "hello"}, {"text": "world"}]}))
        for pk, kind, meta in [(100, "audio", b"\x02AUDIO\0"), (101, "drawing", b'\x01{"indexableContent":"  handwritten  "}'), (102, "unknownFutureType", b'\x01{"latitude":25,"longitude":121}')]:
            con.execute("insert into ZJOURNALENTRYASSETMO(Z_PK,ZENTRY,ZASSETTYPE,ZASSETMETADATA) values(?,1,?,?)", (pk, kind, meta))
    commands = [["list"], ["list", "--include-empty"], ["list", "--limit", "2"], ["list", "--since", "2024-01-01", "--until", "2024-12-31"], ["search", "CAFÉ"], ["search", "台灣"], ["search", "no results"], ["journals"], ["deleted"]]
    commands += [["show", str(pk)] for pk in [1, 2, 3, 4, 5, 6, 99, 100]]
    for command in commands:
        got = [json.loads(run(b, "--db", db, *command, "--json")) for b in [reference, rust]]
        assert got[0] == got[1], (command, got)
        checks += 1
    assert run(reference,"--db",db,"stats") == run(rust,"--db",db,"stats")
    checks += 1
    outputs = []
    for name, binary in [("reference", reference), ("rust", rust)]:
        dest = d / name
        run(binary, "--db", db, "export", "--format", "json", "--dir", dest)
        outputs.append(json.loads((dest / "journal.json").read_text()))
    assert outputs[0] == outputs[1]
    checks += 1
    for md in ["# Heading", "***both***", "- alpha\n- beta", "1. one\n2. two", "\\*literal\\*", "Use `**lit**`", "日本語 🏕️", "a\ue000b", "foo__bar__baz", "[site](https://example.com)", "![alt](https://example.com/i.png)", "first\r\nsecond", "```\n# literal\n```"]:
        for flags in [[], ["--plain"], ["--inline"]]:
            got = [run(b, "render", "--body", md, *flags) for b in [reference, rust]]
            assert got[0] == got[1], (md, flags, got)
            checks += 1
print(f"{checks} differential checks passed")
