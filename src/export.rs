//! Export serialization and filesystem effects, separate from terminal presentation.
use crate::{
    cli::{ExportFormat, ExportRequest},
    model::{self, EntryDetail},
    response::Response,
    store,
};
use anyhow::Result;
use std::{fs, path::Path};

pub fn run(path: &Path, request: &ExportRequest) -> Result<Response> {
    let snapshot = store::Snapshot::new(path)?;
    let entries = model::fetch(&snapshot.db, false)?
        .into_iter()
        .map(|entry| {
            Ok(EntryDetail {
                assets: model::assets(&snapshot.db, path, entry.id)?,
                entry,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    fs::create_dir_all(&request.dir)?;
    let output = match request.format {
        ExportFormat::Json => {
            let output = request.dir.join("journal.json");
            fs::write(&output, serde_json::to_vec_pretty(&entries)?)?;
            output
        }
        ExportFormat::Md => {
            for e in &entries {
                let (date, id) = (e.entry.date.as_deref().unwrap_or(""), e.entry.id);
                let slug = e
                    .entry
                    .title
                    .chars()
                    .filter(|c| c.is_alphanumeric() || "-_ ".contains(*c))
                    .take(50)
                    .collect::<String>()
                    .trim()
                    .replace(' ', "-");
                let name = format!(
                    "{}-{id}{}.md",
                    date.get(..10).unwrap_or(""),
                    if slug.is_empty() {
                        String::new()
                    } else {
                        format!("-{slug}")
                    }
                );
                fs::write(request.dir.join(name), markdown(e)?)?;
            }
            request.dir.clone()
        }
    };
    Ok(Response::Export {
        entries: entries.len(),
        path: output,
    })
}
fn markdown(detail: &EntryDetail) -> Result<String> {
    let e = &detail.entry;
    let mut out = format!(
        "---\ndate: {}\nid: {}\n",
        e.date.as_deref().unwrap_or(""),
        e.id
    );
    if !e.title.is_empty() {
        out += &format!("title: {}\n", serde_json::to_string(&e.title)?);
    }
    if e.bookmarked {
        out += "bookmarked: true\n";
    }
    let places = detail
        .assets
        .iter()
        .flat_map(|a| a.places())
        .collect::<Vec<_>>();
    if !places.is_empty() {
        out += "locations:\n";
        for p in places {
            out += &format!(
                "  - name: {}\n    lat: {}\n    lon: {}\n",
                serde_json::to_string(&p.name)?,
                p.lat,
                serde_json::to_string(&p.lon)?
            );
        }
    }
    out += &format!("---\n\n{}\n", e.text);
    let files = detail
        .assets
        .iter()
        .flat_map(|a| &a.files)
        .collect::<Vec<_>>();
    if !files.is_empty() {
        out.push('\n');
        for f in files {
            out += &format!(
                "![{}]({})\n",
                f.name.as_deref().unwrap_or("file"),
                f.path.display()
            );
        }
    }
    Ok(out)
}
