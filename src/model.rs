//! Typed domain records and Core Data decoding. SQLite types are checked at read time.
use crate::{codec, store};
use anyhow::{Context, Result};
use rusqlite::{Connection, Params, Row};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(crate) fn query<T>(
    db: &Connection,
    sql: &str,
    p: impl Params,
    map: impl FnMut(&Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>> {
    Ok(db
        .prepare(sql)?
        .query_map(p, map)?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
pub(crate) fn ids(db: &Connection, sql: &str, p: impl Params) -> Result<Vec<i64>> {
    query(db, sql, p, |r| r.get(0))
}
pub(crate) fn flag(r: &Row<'_>, column: &str) -> rusqlite::Result<bool> {
    Ok(r.get::<_, Option<i64>>(column)?.unwrap_or(0) != 0)
}
#[derive(Serialize, Debug)]
pub struct Entry {
    pub id: i64,
    pub uuid: Option<String>,
    pub date: Option<String>,
    pub title: String,
    pub text: String,
    pub chars: i64,
    pub bookmarked: bool,
    pub draft: bool,
    pub synced: bool,
    #[serde(skip)]
    pub timestamp: Option<f64>,
}
struct StoredEntry {
    id: i64,
    uuid: Option<Vec<u8>>,
    timestamp: Option<f64>,
    title: Option<Vec<u8>>,
    text: Option<Vec<u8>>,
    chars: i64,
    bookmarked: bool,
    draft: bool,
    synced: bool,
}
impl StoredEntry {
    fn read(r: &Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get("Z_PK")?,
            uuid: r.get("ZID")?,
            timestamp: r.get("ZENTRYDATE")?,
            title: r.get("ZTITLE")?,
            text: r.get("ZTEXT")?,
            chars: r.get::<_, Option<i64>>("ZTEXTLENGTH")?.unwrap_or(0),
            bookmarked: flag(r, "ZFLAGGED")?,
            draft: flag(r, "ZISDRAFT")?,
            synced: flag(r, "ZISUPLOADEDTOCLOUD")?,
        })
    }
    fn decode(self) -> Result<Entry> {
        Ok(Entry {
            id: self.id,
            uuid: store::uuid(self.uuid.as_deref()),
            date: store::date(self.timestamp),
            title: codec::decode(self.title.as_deref())?,
            text: codec::decode(self.text.as_deref())?,
            chars: self.chars,
            bookmarked: self.bookmarked,
            draft: self.draft,
            synced: self.synced,
            timestamp: self.timestamp,
        })
    }
}
const ENTRY_COLUMNS: &str =
    "Z_PK,ZID,ZENTRYDATE,ZTITLE,ZTEXT,ZTEXTLENGTH,ZFLAGGED,ZISDRAFT,ZISUPLOADEDTOCLOUD";
pub(crate) fn fetch(db: &Connection, include_empty: bool) -> Result<Vec<Entry>> {
    let rows = query(
        db,
        &format!(
            "select {ENTRY_COLUMNS} from ZJOURNALENTRYMO where coalesce(ZISFULLYREMOVED,0)=0 and coalesce(ZRECENTLYDELETED,0)=0 order by ZENTRYDATE desc"
        ),
        [],
        StoredEntry::read,
    )?;
    let mut entries = Vec::with_capacity(rows.len());
    for row in rows {
        let entry = row.decode()?;
        if entry.date.is_some()
            && (include_empty || !entry.text.is_empty() || !entry.title.is_empty())
        {
            entries.push(entry);
        }
    }
    Ok(entries)
}
pub(crate) fn fetch_one(db: &Connection, id: i64) -> Result<Entry> {
    db.query_row(
        &format!("select {ENTRY_COLUMNS} from ZJOURNALENTRYMO where Z_PK=?"),
        [id],
        StoredEntry::read,
    )
    .with_context(|| format!("no entry with id {id}"))?
    .decode()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Place {
    pub name: Option<String>,
    pub lat: f64,
    pub lon: Option<f64>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Visit {
    pub name: Option<String>,
    pub city: Option<String>,
    pub lat: f64,
    pub lon: Option<f64>,
}
#[derive(Debug, Serialize)]
pub struct Attachment {
    pub path: PathBuf,
    pub name: Option<String>,
    pub exists: bool,
}
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum AssetDetails {
    Map {
        places: Vec<Visit>,
    },
    Audio {
        #[serde(skip_serializing_if = "Option::is_none")]
        duration: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        transcript: Option<String>,
    },
    Drawing {
        #[serde(skip_serializing_if = "Option::is_none")]
        drawing_text: Option<String>,
    },
    Link {
        #[serde(skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        link_title: Option<String>,
    },
    Other {
        #[serde(skip_serializing_if = "Option::is_none")]
        place: Option<Place>,
        #[serde(skip)]
        raw: Value,
    },
}
#[derive(Debug, Serialize)]
pub struct Asset {
    pub id: i64,
    pub uuid: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub source: Option<String>,
    #[serde(flatten)]
    pub details: AssetDetails,
    pub files: Vec<Attachment>,
}
impl Asset {
    pub fn places(&self) -> &[Visit] {
        match &self.details {
            AssetDetails::Map { places } => places,
            _ => &[],
        }
    }
}
#[derive(Debug, Serialize)]
pub struct EntryDetail {
    #[serde(flatten)]
    pub entry: Entry,
    pub assets: Vec<Asset>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    #[serde(default)]
    visits_data: Vec<VisitMetadata>,
    duration: Option<f64>,
    #[serde(default)]
    transcript_segments: Vec<Transcript>,
    indexable_content: Option<String>,
    data: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisitMetadata {
    place_name: Option<String>,
    city: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}
#[derive(Deserialize)]
struct Transcript {
    text: Option<String>,
}
// Codable metadata field names are Apple format names; Rust's public models use snake_case.
pub(crate) fn metadata(blob: Option<&[u8]>, path: &Path) -> Result<Value> {
    let Some(b) = blob.filter(|b| !b.is_empty()) else {
        return Ok(json!({}));
    };
    if b[0] == 2 {
        let reference = std::str::from_utf8(&b[1..])?.trim_end_matches('\0');
        anyhow::ensure!(
            !reference.contains('/') && reference != ".." && !reference.is_empty(),
            "invalid external metadata reference"
        );
        let file = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(".moments_SUPPORT/_EXTERNAL_DATA")
            .join(reference);
        return match fs::read(&file) {
            Ok(bytes) => {
                Ok(serde_json::from_slice(&bytes).context("invalid external metadata JSON")?)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({"ref":reference})),
            Err(e) => Err(e).with_context(|| format!("cannot read metadata {}", file.display())),
        };
    }
    serde_json::from_slice(if b[0] == 1 { &b[1..] } else { b })
        .context("invalid asset metadata JSON")
}
pub(crate) fn assets(db: &Connection, path: &Path, id: i64) -> Result<Vec<Asset>> {
    struct StoredAsset {
        id: i64,
        uuid: Option<Vec<u8>>,
        kind: String,
        source: Option<String>,
        metadata: Option<Vec<u8>>,
    }
    let rows = query(
        db,
        "select Z_PK,ZID,ZASSETTYPE,ZSOURCE,ZASSETMETADATA from ZJOURNALENTRYASSETMO where ZENTRY=? order by Z_PK",
        [id],
        |r| {
            Ok(StoredAsset {
                id: r.get(0)?,
                uuid: r.get(1)?,
                kind: r.get::<_, Option<String>>(2)?.unwrap_or_else(|| "?".into()),
                source: r.get(3)?,
                metadata: r.get(4)?,
            })
        },
    )?;
    rows.into_iter()
        .map(|row| {
            let raw = metadata(row.metadata.as_deref(), path)
                .with_context(|| format!("asset {} metadata", row.id))?;
            let details = asset_details(&row.kind, raw)
                .with_context(|| format!("asset {} has invalid metadata fields", row.id))?;
            Ok(Asset {
                id: row.id,
                uuid: store::uuid(row.uuid.as_deref()),
                kind: row.kind,
                source: row.source,
                details,
                files: attachment_files(db, path, row.id)?,
            })
        })
        .collect()
}
fn asset_details(kind: &str, raw: Value) -> Result<AssetDetails> {
    // Unknown asset types retain their metadata without imposing another type's schema.
    if !matches!(
        kind,
        "multiPinMap" | "genericMap" | "audio" | "drawing" | "link"
    ) {
        let place = raw["latitude"].as_f64().map(|lat| Place {
            name: raw["placeName"].as_str().map(String::from),
            lat,
            lon: raw["longitude"].as_f64(),
        });
        return Ok(AssetDetails::Other { place, raw });
    }
    let m: Metadata = serde_json::from_value(raw)?;
    Ok(match kind {
        "multiPinMap" | "genericMap" => AssetDetails::Map {
            places: m
                .visits_data
                .into_iter()
                .filter_map(|v| {
                    v.latitude.map(|lat| Visit {
                        name: v.place_name,
                        city: v.city,
                        lat,
                        lon: v.longitude,
                    })
                })
                .collect(),
        },
        "audio" => {
            let text = m
                .transcript_segments
                .into_iter()
                .filter_map(|s| s.text)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            AssetDetails::Audio {
                duration: m.duration,
                transcript: (!text.is_empty()).then_some(text),
            }
        }
        "drawing" => AssetDetails::Drawing {
            drawing_text: m
                .indexable_content
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
        },
        "link" => {
            let decoded = if let Some(data) = m.data {
                codec::bridge(json!({"op":"link-decode", "text":data}))?
            } else {
                json!({})
            };
            AssetDetails::Link {
                url: decoded["url"].as_str().map(String::from),
                link_title: decoded["link_title"].as_str().map(String::from),
            }
        }
        _ => unreachable!("unknown types returned above"),
    })
}
fn attachment_files(db: &Connection, path: &Path, asset: i64) -> Result<Vec<Attachment>> {
    let rows = query(
        db,
        "select ZFILEPATH,ZNAME from ZJOURNALENTRYASSETFILEATTACHMENTMO where ZASSET=? order by ZINDEX",
        [asset],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
            ))
        },
    )?;
    Ok(rows
        .into_iter()
        .filter_map(|(file, name)| {
            let file = file.filter(|s| !s.is_empty())?;
            let full = if Path::new(&file).is_absolute() {
                file.into()
            } else {
                store::attachments(path).join(file)
            };
            Some(Attachment {
                exists: full.exists(),
                path: full,
                name,
            })
        })
        .collect())
}

pub(crate) fn detail(db: &Connection, path: &Path, id: i64) -> Result<EntryDetail> {
    Ok(EntryDetail {
        entry: fetch_one(db, id)?,
        assets: assets(db, path, id)?,
    })
}
#[derive(Debug, Serialize)]
pub struct Journal {
    pub pk: i64,
    pub name: String,
    #[serde(rename = "default")]
    pub is_default: bool,
}
pub(crate) fn journals(db: &Connection) -> Result<Vec<Journal>> {
    query(
        db,
        "select Z_PK,ZMERGEABLEATTRIBUTES,ZSORTCATEGORY from ZJOURNALMO where coalesce(ZUSERDELETED,0)=0 order by Z_PK",
        [],
        |r| {
            let blob = r.get::<_, Option<Vec<u8>>>(1)?;
            let category = r.get::<_, Option<f64>>(2)?;
            let mut name = "Journal".to_owned();
            if let Some(b) = &blob {
                let runs = b
                    .split(|b| !(0x20..0x7f).contains(b))
                    .filter(|s| s.len() >= 3)
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .collect::<Vec<_>>();
                if let Some(i) = runs.iter().position(|s| s == "title").filter(|i| *i > 0) {
                    name = runs[i - 1].clone();
                }
            }
            Ok(Journal {
                pk: r.get(0)?,
                name,
                is_default: blob.is_none() && category.is_some_and(|n| n < 0.),
            })
        },
    )
}
pub(crate) fn resolve_journal(db: &Connection, s: &str) -> Result<Journal> {
    let hits = journals(db)?
        .into_iter()
        .filter(|j| {
            if let Ok(pk) = s.parse::<i64>() {
                j.pk == pk
            } else {
                j.name.to_lowercase() == s.to_lowercase()
            }
        })
        .collect::<Vec<_>>();
    anyhow::ensure!(
        hits.len() == 1,
        "journal '{s}' is missing or ambiguous; use an id from journals"
    );
    Ok(hits.into_iter().next().unwrap())
}
