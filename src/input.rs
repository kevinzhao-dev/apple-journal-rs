//! Prepare text/media before opening a write transaction.
use crate::{
    cli::{EntryOptions, MapSize, MediaOptions, TextSource},
    codec,
};
use anyhow::{Context, Result};
use serde_json::json;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
pub const PHOTOS: &[&str] = &["jpg", "jpeg", "heic", "heif", "png", "gif", "tiff", "webp"];
pub const VIDEOS: &[&str] = &["mov", "mp4", "m4v", "avi"];
pub fn ext(p: &Path) -> String {
    p.extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
}
pub fn read_body(body: Option<&str>, file: Option<&Path>) -> Result<Option<String>> {
    if let Some(body) = body {
        return Ok(Some(body.into()));
    }
    if let Some(file) = file {
        return Ok(Some(
            fs::read_to_string(file).with_context(|| format!("cannot read {}", file.display()))?,
        ));
    }
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // fstat initializes stat only on success. Pipes/regular files cannot prompt a user.
    if unsafe { libc::fstat(0, stat.as_mut_ptr()) } == 0 {
        let mode = unsafe { stat.assume_init() }.st_mode & libc::S_IFMT;
        if mode == libc::S_IFIFO || mode == libc::S_IFREG {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s)?;
            if !s.is_empty() {
                return Ok(Some(s));
            }
        }
    }
    Ok(None)
}
#[derive(Debug)]
pub struct PreparedText {
    pub plain: String,
    pub rtf: Vec<u8>,
}
impl PreparedText {
    pub fn read(source: &TextSource) -> Result<Option<Self>> {
        if let Some(path) = &source.body_rtf {
            let rtf = fs::read(path)?;
            anyhow::ensure!(
                codec::valid_rtf(&rtf)?,
                "--body-rtf is not valid RTF: {}",
                path.display()
            );
            return Ok(Some(Self {
                plain: codec::decode(Some(&rtf))?,
                rtf,
            }));
        }
        read_body(source.body.as_deref(), source.body_file.as_deref())?
            .map(|text| {
                let (rtf, decoded) = codec::encode(&text, source.markdown)?;
                Ok(Self {
                    plain: if source.markdown { decoded } else { text },
                    rtf,
                })
            })
            .transpose()
    }
}
#[derive(Debug)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub place: Option<String>,
    pub city: Option<String>,
    pub size: MapSize,
}
#[derive(Debug)]
pub struct PreparedEntry {
    pub body: Option<PreparedText>,
    pub title: Option<String>,
    pub media: Vec<PathBuf>,
    pub pair: Option<[PathBuf; 2]>,
    pub location: Option<Location>,
    pub link: Option<String>,
    pub date: Option<f64>,
    pub journal: Option<String>,
}
impl PreparedEntry {
    pub fn new(
        entry: &EntryOptions,
        media: &[PathBuf],
        pair: &[PathBuf],
        link: Option<&str>,
        options: &MediaOptions,
    ) -> Result<Self> {
        let body = PreparedText::read(&entry.text)?;
        let title = entry
            .title
            .as_deref()
            .map(|s| {
                if entry.text.markdown {
                    codec::inline(s)
                } else {
                    Ok(s.into())
                }
            })
            .transpose()?;
        let pair = if pair.is_empty() {
            None
        } else {
            anyhow::ensure!(
                pair.len() == 2
                    && PHOTOS.contains(&ext(&pair[0]).as_str())
                    && VIDEOS.contains(&ext(&pair[1]).as_str()),
                "--live-photo takes IMAGE then VIDEO"
            );
            Some([pair[0].clone(), pair[1].clone()])
        };
        for p in media.iter().chain(pair.iter().flatten()) {
            anyhow::ensure!(p.is_file(), "media file not found: {}", p.display());
            anyhow::ensure!(
                PHOTOS.contains(&ext(p).as_str()) || VIDEOS.contains(&ext(p).as_str()),
                "unsupported media type: {}",
                p.display()
            );
        }
        let location = match (entry.location.lat, entry.location.lon) {
            (None, None) => None,
            (Some(lat), Some(lon)) => {
                anyhow::ensure!(
                    lat.is_finite()
                        && lon.is_finite()
                        && (-90.0..=90.).contains(&lat)
                        && (-180.0..=180.).contains(&lon),
                    "invalid latitude or longitude"
                );
                Some(Location {
                    lat,
                    lon,
                    place: entry.location.place.clone(),
                    city: entry.location.city.clone(),
                    size: entry.location.location_presentation.unwrap_or_default(),
                })
            }
            _ => anyhow::bail!("--lat and --lon must be given together"),
        };
        let link = link
            .map(|url| {
                let value = codec::bridge(
                    json!({"op":"link-encode","text":url,"title":options.link_title}),
                )?;
                Ok::<_, anyhow::Error>(
                    value["data"]
                        .as_str()
                        .context("missing link archive")?
                        .to_owned(),
                )
            })
            .transpose()?;
        Ok(Self {
            body,
            title,
            media: media.to_vec(),
            pair,
            location,
            link,
            date: entry.date,
            journal: entry.journal.clone(),
        })
    }
    pub fn has_content(&self) -> bool {
        self.body
            .as_ref()
            .is_some_and(|b| !b.plain.trim().is_empty())
            || self.title.as_ref().is_some_and(|s| !s.trim().is_empty())
            || !self.media.is_empty()
            || self.pair.is_some()
            || self.location.is_some()
            || self.link.is_some()
    }
    pub fn has_changes(&self) -> bool {
        self.body.is_some()
            || self.title.is_some()
            || !self.media.is_empty()
            || self.pair.is_some()
            || self.location.is_some()
            || self.link.is_some()
            || self.date.is_some()
            || self.journal.is_some()
    }
}
