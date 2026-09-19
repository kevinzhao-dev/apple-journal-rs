#!/usr/bin/env python3
"""Render LLVM's measured Rust coverage; do not present it as branch coverage."""
import json
import pathlib
import sys

report = json.loads(pathlib.Path(sys.argv[1]).read_text())["data"][0]
root = pathlib.Path(__file__).resolve().parents[1]
print("# Rust coverage\n")
print("Synthetic unit, integration, upstream and differential tests; CLI subprocesses included.\n")
print("| File | Lines | Functions | Regions |")
print("| --- | ---: | ---: | ---: |")

def row(name, summary):
    values = []
    for metric in ("lines", "functions", "regions"):
        v = summary[metric]
        values.append(f'{v["percent"]:.2f}% ({v["covered"]}/{v["count"]})')
    print(f'| {name} | ' + ' | '.join(values) + ' |')

for file in sorted(report["files"], key=lambda f: f["filename"]):
    path = pathlib.Path(file["filename"])
    row(str(path.relative_to(root)) if path.is_relative_to(root) else path.name, file["summary"])
row("**Total**", report["totals"])
print("\nExcludes tests, build.rs, dependencies and the Swift bridge. Stable Rust line/region coverage is not branch coverage.")
