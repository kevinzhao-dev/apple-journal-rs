//! Transaction-local operations and attachment lifetime management.
use crate::{
    cli::{CreateRequest, DeleteRequest, EditRequest, MapSize, MediaOptions},
    codec,
    input::{PHOTOS, PreparedEntry, VIDEOS, ext},
    model::{self, ids, query},
    response::{MutationEffects, MutationResult},
    store,
};
use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub(crate) struct Files {
    created: Vec<PathBuf>,
    remove: Vec<PathBuf>,
    committed: bool,
}
impl Files {
    pub(crate) fn finish(mut self) -> Vec<String> {
        self.committed = true;
        let mut warnings = vec![];
        for path in &self.remove {
            if path.exists()
                && let Err(error) = fs::remove_dir_all(path)
            {
                warnings.push(format!(
                    "database committed, could not remove {}: {error}",
                    path.display()
                ));
            }
        }
        warnings
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        if !self.committed {
            for p in self.created.iter().rev() {
                let _ = fs::remove_file(p);
            }
        }
    }
}
pub(crate) struct Mutation<'a> {
    db: &'a Connection,
    path: &'a Path,
    files: Files,
    effects: MutationEffects,
}
struct AssetContext<'a> {
    id: i64,
    uuid: &'a str,
    entry_date: f64,
    media_date: f64,
    inherit_location: bool,
}
impl<'a> Mutation<'a> {
    pub(crate) fn new(db: &'a Connection, path: &'a Path, backup: Option<PathBuf>) -> Self {
        Self {
            db,
            path,
            files: Files::default(),
            effects: MutationEffects {
                backup,
                ..MutationEffects::default()
            },
        }
    }
    pub(crate) fn into_effects(self) -> (Files, MutationEffects) {
        (self.files, self.effects)
    }
    fn next(&self, name: &str) -> Result<(i64, i64)> {
        let (ent, max): (i64, i64) = self
            .db
            .query_row(
                "select Z_ENT,Z_MAX from Z_PRIMARYKEY where Z_NAME=?",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .with_context(|| format!("invalid primary key bookkeeping for {name}"))?;
        let pk = max.checked_add(1).context("primary key overflow")?;
        self.db.execute(
            "update Z_PRIMARYKEY set Z_MAX=? where Z_NAME=?",
            params![pk, name],
        )?;
        Ok((ent, pk))
    }
    fn add_asset(
        &self,
        entry: &AssetContext<'_>,
        kind: &str,
        source: &str,
        meta: Value,
        slim: i64,
    ) -> Result<(i64, String)> {
        let (ent, id) = self.next("JournalEntryAssetMO")?;
        let uuid = store::uid();
        let mut blob = vec![1];
        blob.extend(serde_json::to_vec(&meta)?);
        self.db.execute("insert into ZJOURNALENTRYASSETMO (Z_PK,Z_ENT,Z_OPT,ZENTRY,ZID,ZPARENTID,ZASSETTYPE,ZSOURCE,ZCREATEDDATE,ZASSETMETADATA,ZISSLIM,ZISHIDDEN,ZISBEINGEDITED,ZISUNDOABLYDELETED,ZISUPLOADEDTOCLOUD,ZISREMOVEDFROMCLOUD,ZREFRESHASSETMETADATA,ZMINIMUMSUPPORTEDAPPVERSION) values (?,?,1,?,?,?,?,?,?,?,?,0,0,0,0,0,0,0)",params![id,ent,entry.id,store::uuid_bytes(&uuid)?,store::uuid_bytes(entry.uuid)?,kind,source,store::now(),blob,slim])?;
        Ok((id, uuid))
    }
    fn add_file(
        &mut self,
        entry: &AssetContext<'_>,
        asset: (i64, &str),
        source: &Path,
        resize: bool,
        live_photo: bool,
    ) -> Result<()> {
        let extension = ext(source);
        let image = PHOTOS.contains(&extension.as_str());
        let relative = PathBuf::from(entry.uuid).join(asset.1).join(format!(
            "{}{}.{}",
            store::uid(),
            if live_photo { "" } else { "_resized" },
            extension
        ));
        let dest = store::attachments(self.path).join(&relative);
        fs::create_dir_all(dest.parent().unwrap())?;
        self.files.created.push(dest.clone());
        if image && resize && !live_photo {
            codec::bridge(json!({"op":"resize","text":source,"dest":dest}))?;
        } else {
            fs::copy(source, &dest)?;
        }
        let (ent, id) = self.next("JournalEntryAssetFileAttachmentMO")?;
        self.db.execute("insert into ZJOURNALENTRYASSETFILEATTACHMENTMO (Z_PK,Z_ENT,Z_OPT,ZASSET,ZID,ZPARENTID,ZFILEPATH,ZNAME,ZINDEX,ZISUPLOADEDTOCLOUD,ZISREMOVEDFROMCLOUD) values (?,?,1,?,?,?,?,?,0,0,0)",params![id,ent,asset.0,store::uuid_bytes(&store::uid())?,store::uuid_bytes(asset.1)?,relative.to_string_lossy(),if image{"image"}else{"video"}])?;
        Ok(())
    }
    fn ordering(&self, id: i64, added: &[String], gone: &HashSet<String>) -> Result<()> {
        let blob: Option<Vec<u8>> = self.db.query_row(
            "select ZASSETORDERING from ZJOURNALENTRYMO where Z_PK=?",
            [id],
            |r| r.get(0),
        )?;
        let current: Vec<Value> = blob
            .as_deref()
            .map(serde_json::from_slice)
            .transpose()
            .context("invalid asset ordering")?
            .unwrap_or_default();
        anyhow::ensure!(
            current.len().is_multiple_of(2),
            "invalid asset ordering pairs"
        );
        let (mut out, mut n) = (vec![], 0);
        for pair in current.chunks_exact(2) {
            let uuid = pair[0].as_str().context("invalid asset ordering UUID")?;
            let index = pair[1].as_i64().context("invalid asset ordering index")?;
            if !gone.contains(uuid) {
                out.extend_from_slice(pair);
                n = n.max(index.checked_add(1).context("ordering index overflow")?);
            }
        }
        for uuid in added {
            out.push(json!(uuid));
            out.push(json!(n));
            n = n.checked_add(1).context("ordering index overflow")?;
        }
        self.db.execute(
            "update ZJOURNALENTRYMO set ZASSETORDERING=? where Z_PK=?",
            params![serde_json::to_vec(&out)?, id],
        )?;
        Ok(())
    }
    fn add_assets(
        &mut self,
        input: &PreparedEntry,
        entry: &AssetContext<'_>,
        options: &MediaOptions,
    ) -> Result<()> {
        let mut added = vec![];
        for source in &input.media {
            let mut meta = json!({"date":entry.media_date});
            if entry.inherit_location
                && let Some(loc) = &input.location
            {
                meta["latitude"] = json!(loc.lat);
                meta["longitude"] = json!(loc.lon);
                if let Some(name) = &loc.place {
                    meta["placeName"] = json!(name);
                }
            }
            if options.photos_link {
                if let Some(id) = photos_lookup(source) {
                    meta["assetIdentifier"] = json!(id);
                } else {
                    self.effects
                        .warnings
                        .push(format!("no Photos-library match for {}", source.display()));
                }
            }
            let (id, uuid) = self.add_asset(
                entry,
                if VIDEOS.contains(&ext(source).as_str()) {
                    "video"
                } else {
                    "photo"
                },
                "imagePicker",
                meta,
                0,
            )?;
            self.add_file(entry, (id, &uuid), source, !options.no_resize, false)?;
            added.push(uuid);
        }
        if let Some(pair) = &input.pair {
            let mut meta = json!({"date":entry.media_date});
            if options.photos_link
                && let Some(id) = photos_lookup(&pair[0])
            {
                meta["assetIdentifier"] = json!(id);
            }
            let (id, uuid) = self.add_asset(entry, "livePhoto", "imagePicker", meta, 0)?;
            for source in pair {
                self.add_file(entry, (id, &uuid), source, false, true)?;
            }
            added.push(uuid);
        }
        if let Some(link) = &input.link {
            let (id, uuid) = self.add_asset(
                entry,
                "link",
                "shareSheet",
                json!({"data":link,"date":store::now()}),
                0,
            )?;
            self.db.execute(
                "update ZJOURNALENTRYASSETMO set ZCONTENTTYPE='unknown' where Z_PK=?",
                [id],
            )?;
            added.push(uuid);
        }
        if let Some(loc) = &input.location {
            let mut visit = json!({"latitude":loc.lat,"longitude":loc.lon,"createdDate":store::now(),"visitStartTime":entry.entry_date,"visitEndTime":entry.entry_date,"horizontalAccuracy":0,"confidenceLevel":0,"assetSource":"locationPicker"});
            if let Some(s) = &loc.place {
                visit["placeName"] = json!(s);
            }
            if let Some(s) = &loc.city {
                visit["city"] = json!(s);
            }
            let (_, uuid) = self.add_asset(
                entry,
                "multiPinMap",
                "locationPicker",
                json!({"revision":2,"visitsData":[visit]}),
                loc.size.slim(),
            )?;
            added.push(uuid);
        }
        if !added.is_empty() {
            self.ordering(entry.id, &added, &HashSet::new())?;
        }
        Ok(())
    }
    fn stage(&mut self, id: i64, selector: Option<&str>) -> Result<()> {
        if let Some(selector) = selector {
            let journal = model::resolve_journal(self.db, selector)?;
            self.db
                .execute("delete from Z_5JOURNALS where Z_5ENTRIES=?", [id])?;
            if !journal.is_default {
                self.db.execute(
                    "insert into Z_5JOURNALS (Z_5ENTRIES,Z_6JOURNALS) values (?,?)",
                    params![id, journal.pk],
                )?;
                self.effects.staged = true;
            }
        }
        Ok(())
    }
    fn purge(&mut self, id: i64) -> Result<()> {
        let uuid: Option<Vec<u8>> =
            self.db
                .query_row("select ZID from ZJOURNALENTRYMO where Z_PK=?", [id], |r| {
                    r.get(0)
                })?;
        self.db.execute("delete from ZJOURNALENTRYASSETFILEATTACHMENTMO where ZASSET in (select Z_PK from ZJOURNALENTRYASSETMO where ZENTRY=?)",[id])?;
        self.db
            .execute("delete from ZJOURNALENTRYASSETMO where ZENTRY=?", [id])?;
        self.db
            .execute("delete from Z_5JOURNALS where Z_5ENTRIES=?", [id])?;
        self.db
            .execute("delete from ZJOURNALENTRYMO where Z_PK=?", [id])?;
        if let Some(uuid) = store::uuid(uuid.as_deref()) {
            self.files
                .remove
                .push(store::attachments(self.path).join(uuid));
        }
        Ok(())
    }
    fn remove_assets(
        &mut self,
        entry_id: i64,
        entry_uuid: &str,
        ids: &[i64],
        media_only: bool,
    ) -> Result<()> {
        let mut gone = HashSet::new();
        for id in ids {
            let (uuid, kind): (Option<Vec<u8>>, String) = self.db.query_row(
                "select ZID,ZASSETTYPE from ZJOURNALENTRYASSETMO where Z_PK=? and ZENTRY=?",
                params![id, entry_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            anyhow::ensure!(
                !media_only || ["photo", "video", "livePhoto"].contains(&kind.as_str()),
                "asset {id} is not media; refusing to remove it"
            );
            self.db.execute(
                "delete from ZJOURNALENTRYASSETFILEATTACHMENTMO where ZASSET=?",
                [id],
            )?;
            self.db
                .execute("delete from ZJOURNALENTRYASSETMO where Z_PK=?", [id])?;
            if let Some(uuid) = store::uuid(uuid.as_deref()) {
                self.files
                    .remove
                    .push(store::attachments(self.path).join(entry_uuid).join(&uuid));
                gone.insert(uuid);
            }
        }
        if !gone.is_empty() {
            self.ordering(entry_id, &[], &gone)?;
        }
        Ok(())
    }
    pub(crate) fn create(
        &mut self,
        input: &PreparedEntry,
        a: &CreateRequest,
    ) -> Result<MutationResult> {
        let (ent, id) = self.next("JournalEntryMO")?;
        let uuid = store::uid();
        let ts = input.date.unwrap_or_else(store::now);
        let title = input
            .title
            .as_ref()
            .map(|s| codec::encode(s, false).map(|p| p.0))
            .transpose()?;
        let text = input.body.as_ref().map(|b| b.plain.as_str()).unwrap_or("");
        let rtf = input
            .body
            .as_ref()
            .filter(|b| !b.plain.trim().is_empty())
            .map(|b| b.rtf.as_slice());
        let chars = text.graphemes(true).count();
        self.db.execute("insert into ZJOURNALENTRYMO (Z_PK,Z_ENT,Z_OPT,ZENTRYTYPE,ZID,ZENTRYDATE,ZCREATEDDATE,ZUPDATEDDATE,ZMOMENTDATEFORSORTING,ZTEXT,ZTITLE,ZTEXTLENGTH,ZSHOWTITLE,ZISDRAFT,ZFLAGGED,ZISUPLOADEDTOCLOUD,ZISREMOVEDFROMCLOUD,ZISFULLYREMOVED,ZRECENTLYDELETED,ZISTIP,ZMINIMUMSUPPORTEDAPPVERSION,ZMINIMUMSUPPORTEDAPPVERSIONMODE) values (?,?,1,'blankEntry',?,?,?,?,?,?,?,?,?,0,?,0,0,0,0,0,0,0)",params![id,ent,store::uuid_bytes(&uuid)?,ts,store::now(),store::now(),ts,rtf,title,chars as i64,input.title.is_some(),a.bookmark])?;
        self.add_assets(
            input,
            &AssetContext {
                id,
                uuid: &uuid,
                entry_date: ts,
                media_date: ts,
                inherit_location: true,
            },
            &a.media_options,
        )?;
        self.stage(id, input.journal.as_deref())?;
        Ok(MutationResult::Created {
            id,
            chars,
            has_title: input.title.is_some(),
        })
    }
    pub(crate) fn edit(
        &mut self,
        input: &PreparedEntry,
        a: &EditRequest,
    ) -> Result<MutationResult> {
        let id = a.id;
        // Validate after BEGIN IMMEDIATE: an earlier MCP snapshot may be stale.
        if a.require_active {
            let active: bool = self.db.query_row(
                "select exists(select 1 from ZJOURNALENTRYMO where Z_PK=? and coalesce(ZISFULLYREMOVED,0)=0 and coalesce(ZRECENTLYDELETED,0)=0 and ZENTRYDATE is not null)",
                [id], |row| row.get(0),
            )?;
            anyhow::ensure!(active, "no active entry with id {id}");
        }
        struct State {
            uuid: Vec<u8>,
            merge: Option<Vec<u8>>,
            date: Option<f64>,
        }
        let state = self.db.query_row(
            "select ZID,ZMERGEABLEATTRIBUTES,ZENTRYDATE from ZJOURNALENTRYMO where Z_PK=?",
            [id],
            |r| {
                Ok(State {
                    uuid: r.get(0)?,
                    merge: r.get(1)?,
                    date: r.get(2)?,
                })
            },
        )?;
        let uuid = store::uuid(Some(&state.uuid)).context("entry UUID missing")?;
        anyhow::ensure!(
            !(input.body.is_some() || input.title.is_some()) || state.merge.is_none() || a.force,
            "entry {id} carries a ZMERGEABLEATTRIBUTES CRDT; text edits may be reverted or duplicated on sync. Edit in Journal.app or use --force"
        );
        anyhow::ensure!(
            input.journal.is_none() || state.merge.is_none(),
            "entry {id} has Journal merge attributes; move it in Journal.app"
        );
        if let Some(body) = &input.body {
            self.db.execute(
                "update ZJOURNALENTRYMO set ZTEXT=?,ZTEXTLENGTH=? where Z_PK=?",
                params![
                    if body.plain.trim().is_empty() {
                        None
                    } else {
                        Some(&body.rtf)
                    },
                    body.plain.graphemes(true).count() as i64,
                    id
                ],
            )?;
        }
        if let Some(title) = &input.title {
            let rtf = if title.trim().is_empty() {
                None
            } else {
                Some(codec::encode(title, false)?.0)
            };
            self.db.execute(
                "update ZJOURNALENTRYMO set ZTITLE=?,ZSHOWTITLE=? where Z_PK=?",
                params![rtf, !title.trim().is_empty(), id],
            )?;
        }
        if let Some(ts) = input.date {
            self.db.execute(
                "update ZJOURNALENTRYMO set ZENTRYDATE=?,ZMOMENTDATEFORSORTING=? where Z_PK=?",
                params![ts, ts, id],
            )?;
        }
        if a.bookmark || a.no_bookmark {
            self.db.execute(
                "update ZJOURNALENTRYMO set ZFLAGGED=? where Z_PK=?",
                params![a.bookmark, id],
            )?;
        }
        if a.clear_location || input.location.is_some() {
            let maps = ids(
                self.db,
                "select Z_PK from ZJOURNALENTRYASSETMO where ZENTRY=? and ZASSETTYPE in ('multiPinMap','genericMap')",
                [id],
            )?;
            self.remove_assets(id, &uuid, &maps, false)?;
        }
        let remove = if a.remove_all_media {
            ids(
                self.db,
                "select Z_PK from ZJOURNALENTRYASSETMO where ZENTRY=? and ZASSETTYPE in ('photo','video','livePhoto')",
                [id],
            )?
        } else {
            a.remove_media.clone()
        };
        self.remove_assets(id, &uuid, &remove, true)?;
        // Entry and media timestamps have distinct meanings in the upstream format.
        self.add_assets(
            input,
            &AssetContext {
                id,
                uuid: &uuid,
                entry_date: input.date.or(state.date).unwrap_or_else(store::now),
                media_date: store::now(),
                inherit_location: false,
            },
            &a.media_options,
        )?;
        self.touch(id)?;
        self.stage(id, input.journal.as_deref())?;
        Ok(MutationResult::Updated { id })
    }
    fn touch(&self, id: i64) -> Result<()> {
        self.db.execute("update ZJOURNALENTRYMO set ZUPDATEDDATE=?,ZENTRYDATAUPDATEDATE=?,ZISUPLOADEDTOCLOUD=0 where Z_PK=?",params![store::now(),store::now(),id])?;
        Ok(())
    }
    pub(crate) fn delete(&mut self, a: &DeleteRequest) -> Result<MutationResult> {
        let synced = self.db.query_row(
            "select ZISUPLOADEDTOCLOUD from ZJOURNALENTRYMO where Z_PK=?",
            [a.id],
            |r| model::flag(r, "ZISUPLOADEDTOCLOUD"),
        )?;
        if a.hard {
            anyhow::ensure!(
                !synced || a.force,
                "entry {} has synced to iCloud; local hard deletion can resurrect it. Use soft delete or --force",
                a.id
            );
            self.purge(a.id)?;
        } else {
            self.db.execute("update ZJOURNALENTRYMO set ZRECENTLYDELETED=1,ZRECENTLYDELETEDENTRYDATE=?,ZUPDATEDDATE=?,ZISUPLOADEDTOCLOUD=0 where Z_PK=?",params![store::now(),store::now(),a.id])?;
        }
        Ok(MutationResult::Deleted {
            id: a.id,
            hard: a.hard,
        })
    }
    pub(crate) fn restore(&mut self, id: i64) -> Result<MutationResult> {
        let deleted = self.db.query_row(
            "select ZRECENTLYDELETED from ZJOURNALENTRYMO where Z_PK=?",
            [id],
            |r| model::flag(r, "ZRECENTLYDELETED"),
        )?;
        anyhow::ensure!(deleted, "entry {id} is not in Recently Deleted");
        self.db.execute("update ZJOURNALENTRYMO set ZRECENTLYDELETED=0,ZRECENTLYDELETEDENTRYDATE=NULL,ZUPDATEDDATE=?,ZISUPLOADEDTOCLOUD=0 where Z_PK=?",params![store::now(),id])?;
        Ok(MutationResult::Restored { id })
    }
    pub(crate) fn empty(&mut self, force: bool) -> Result<MutationResult> {
        let (mut purged, mut skipped) = (0, 0);
        for (id, synced) in query(
            self.db,
            "select Z_PK,ZISUPLOADEDTOCLOUD from ZJOURNALENTRYMO where coalesce(ZRECENTLYDELETED,0)=1 and coalesce(ZISFULLYREMOVED,0)=0",
            [],
            |r| Ok((r.get::<_, i64>(0)?, model::flag(r, "ZISUPLOADEDTOCLOUD")?)),
        )? {
            if synced && !force {
                skipped += 1;
            } else {
                self.purge(id)?;
                purged += 1;
            }
        }
        Ok(MutationResult::Emptied { purged, skipped })
    }
    pub(crate) fn repair(&mut self, size: MapSize) -> Result<MutationResult> {
        let rows = query(
            self.db,
            "select Z_PK,ZENTRY from ZJOURNALENTRYASSETMO where ZASSETTYPE='multiPinMap' and coalesce(ZISHIDDEN,0)=1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
        )?;
        for (id, entry) in &rows {
            self.db.execute("update ZJOURNALENTRYASSETMO set ZISHIDDEN=0,ZISSLIM=?,ZISUPLOADEDTOCLOUD=0 where Z_PK=?",params![size.slim(),id])?;
            self.touch(*entry)?;
        }
        Ok(MutationResult::Repaired {
            assets: rows.len(),
            size: size.name(),
        })
    }
}
fn photos_lookup(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if path.to_string_lossy().contains(".photoslibrary/") && uuid::Uuid::parse_str(stem).is_ok() {
        return Some(format!("{}/L0/001", stem.to_uppercase()));
    }
    let db = Connection::open_with_flags(
        store::home().join("Pictures/Photos Library.photoslibrary/database/Photos.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .ok()?;
    let rows=query(&db,"select a.ZUUID,aa.ZORIGINALFILESIZE from ZASSET a join ZADDITIONALASSETATTRIBUTES aa on aa.ZASSET=a.Z_PK where aa.ZORIGINALFILENAME=?",[path.file_name()?.to_str()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Option<i64>>(1)?))).ok()?;
    let hits = if rows.len() == 1 {
        rows.iter().collect::<Vec<_>>()
    } else {
        let size = fs::metadata(path).ok()?.len() as i64;
        rows.iter().filter(|(_, n)| *n == Some(size)).collect()
    };
    if hits.len() == 1 {
        Some(format!("{}/L0/001", hits[0].0))
    } else {
        None
    }
}
