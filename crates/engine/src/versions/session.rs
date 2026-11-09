//! The version history in a [`Session`]: what a save records, and the commands' view of the
//! store (see [`super`] for the store itself and `docs/cloud.md` for the behaviour).
//!
//! GridCraft keeps several workbooks open at once, so everything a workbook needs to remember
//! (its history id, when it was opened) is on its [`crate::DocState`], and the store keys what it
//! remembers per workbook by [`crate::DocState::uid`].
//!
//! A history is written only by a save: a session without a store (the command line, MCP, tests)
//! writes none, and a session with one writes it for `file.save`, `file.saveAs`,
//! `file.saveToCloud` and `file.resolveConflict`, which are the commands that write a
//! workbook's own file. Opening a workbook never writes history.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{
    DeleteScope, DiskBefore, PendingVersion, Relocation, VersionDetails, VersionInfo, VersionKind, VersionOrigin, VersionProperties, VersionStats,
    VersionWrite, Versions, canonical_path, clock, default_device_label, meta, modified_secs, same_file_path,
};
use crate::{DocState, Prefs, Session};

/// The longest device label, in characters.
pub const MAX_DEVICE_LABEL_CHARS: usize = 64;

/// What the history knows about a workbook beyond its bytes: kept on its [`DocState`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DocHistory {
    /// The random id in the file's custom properties (`GridCraftDocumentId`), when the file has
    /// one. Assigned at the first save, kept through every later one, and never derived from the
    /// computer, the user, the time or the path.
    pub id: Option<String>,
    /// When the workbook was opened (seconds since the epoch): where this editing session began.
    pub opened_at: u64,
}

impl DocHistory {
    /// The history of a workbook opened from `bytes`: the id the file carries.
    pub fn opened(bytes: &[u8]) -> DocHistory {
        DocHistory { id: meta::bytes_document_id(bytes), opened_at: clock::now_unix() }
    }
}

/// Whether `path` names a workbook package, whose history is kept under the id in the file
/// (anything else, such as GridCraft's own JSON, is kept under a hash of its path).
pub(crate) fn is_package(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "xlsx" | "xlsm" | "xltx" | "xltm"))
}

/// The id `doc`'s history is kept under when saved at `path`: only packages carry one, and not
/// when the workbook removes personal information.
fn history_id(doc: &DocState, path: Option<&Path>) -> Option<String> {
    if !path.is_some_and(is_package) || doc.wb.remove_personal_info {
        return None;
    }
    doc.history.id.clone().filter(|id| meta::document_id_ok(id))
}

/// The initials of a name, up to three letters.
fn initials(name: &str) -> String {
    name.split_whitespace().filter_map(|word| word.chars().next()).flat_map(char::to_uppercase).take(3).collect()
}

/// The history folders the settings name: the settings folder's alone (the default), another
/// folder instead, or both.
fn version_roots(default: &Path, prefs: &Prefs) -> Vec<PathBuf> {
    match prefs.versions_folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        None => vec![default.to_path_buf()],
        Some(folder) if prefs.versions_mirror => vec![default.to_path_buf(), PathBuf::from(folder)],
        Some(folder) => vec![PathBuf::from(folder)],
    }
}

/// What a save asks of the history before it writes the file.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SaveRequest<'a> {
    /// The save is AutoSave's (`ifUnchanged`).
    pub autosave: bool,
    /// What caused the save, when it says (`close`).
    pub trigger: Option<&'a str>,
    /// The copy Keep both makes after a save conflict.
    pub conflict_copy: bool,
}

impl Session {
    /// Desktop host wiring for the history: `root` is the settings folder's `versions`, the
    /// default; the folders in File › Options › Save apply on top. `wake` runs when the worker
    /// finishes a write, so the host can repaint.
    pub fn enable_versions(&mut self, root: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Result<(), String> {
        let roots = version_roots(&root, &self.prefs);
        self.version_store = Some(Versions::start(root, roots, wake)?);
        Ok(())
    }

    /// Collect what the history worker finished (the host calls it every frame).
    pub fn versions_tick(&mut self) {
        if let Some(store) = self.version_store.as_mut() {
            store.poll();
        }
    }

    /// The last error of the history store, if any (a failed history write never fails a save).
    pub fn versions_error(&mut self) -> Option<String> {
        self.versions_tick();
        self.version_store.as_ref().and_then(|store| store.error.clone())
    }

    /// Whether this session keeps a version history.
    pub fn has_versions(&self) -> bool {
        self.version_store.is_some()
    }

    fn history_doc(&self, uid: u64) -> Option<&DocState> {
        self.documents().iter().find(|d| d.uid == uid)
    }

    /// The id the history of workbook `uid` is kept under: a package's id (none for other
    /// formats, a workbook that removes personal information, or one never saved).
    pub fn version_document_id(&self, uid: u64) -> Option<String> {
        let doc = self.history_doc(uid)?;
        history_id(doc, doc.path.as_deref().map(Path::new))
    }

    /// The versions of the active workbook, newest first.
    pub fn version_infos(&mut self) -> Vec<VersionInfo> {
        self.versions_tick();
        let Some((uid, path)) = self.active().and_then(|d| d.path.clone().map(|p| (d.uid, PathBuf::from(p)))) else { return Vec::new() };
        let id = self.version_document_id(uid);
        let cloud = self.cloud_roots.clone();
        match self.version_store.as_mut() {
            Some(store) => {
                store.ensure_path(&path, id.as_deref(), &cloud);
                store.list.clone()
            }
            None => Vec::new(),
        }
    }

    /// The editing session AutoSave versions are grouped by: this run of the app and this
    /// opening of the workbook.
    fn version_session(&self, uid: u64) -> Option<String> {
        self.version_store.as_ref().map(|store| format!("{}-{uid}", store.run_id))
    }

    /// What a version records about this save (see [`VersionDetails`]), privacy first: no
    /// author or initials when the workbook removes personal information, a random
    /// installation id and the user's device label instead of the computer's name, the path
    /// without the account, and counts instead of contents.
    fn version_details(&self, uid: u64, path: &Path, trigger: &str, before: Option<DiskBefore>, with_stats: bool) -> VersionDetails {
        let now = clock::now_unix();
        let doc = self.history_doc(uid);
        let private = doc.is_some_and(|d| d.wb.remove_personal_info);
        let opened = doc.map_or(0, |d| d.history.opened_at);
        let mut details = VersionDetails {
            utc_offset: clock::local_offset(now),
            session_start: (opened > 0).then(|| clock::iso_from_unix_secs(opened)),
            editing_seconds: (opened > 0).then(|| now.saturating_sub(opened)),
            trigger: trigger.to_owned(),
            initials: if private { String::new() } else { initials(&self.prefs.user_name) },
            installation: self.version_store.as_ref().map(|s| s.installation.clone()).unwrap_or_default(),
            device: self.prefs.device_label.clone().filter(|l| !l.trim().is_empty()).unwrap_or_else(|| default_device_label().to_owned()),
            os: std::env::consts::OS.to_owned(),
            os_version: meta::os_version(),
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            commit: option_env!("GRIDCRAFT_COMMIT").map(str::to_owned),
            path: meta::portable_path(path, &self.cloud_roots),
            provider: meta::provider_of(path, &self.cloud_roots).to_owned(),
            before,
            ..VersionDetails::default()
        };
        if let (true, Some(d)) = (with_stats, doc) {
            let wb = &d.wb;
            details.properties =
                Some(VersionProperties { title: wb.props.title.clone(), creator: if private { String::new() } else { wb.props.author.clone() } });
            let mut stats = VersionStats { sheets: wb.sheets.len() as u64, ..VersionStats::default() };
            for sheet in &wb.sheets {
                stats.used_cells = stats.used_cells.saturating_add(sheet.cells.len() as u64);
                stats.formulas = stats.formulas.saturating_add(sheet.cells.iter().filter(|(_, cell)| cell.formula.is_some()).count() as u64);
                stats.charts = stats.charts.saturating_add(sheet.charts.len() as u64);
                stats.comments = stats.comments.saturating_add(sheet.comments.len() as u64);
            }
            details.stats = Some(stats);
        }
        details
    }

    #[allow(clippy::too_many_arguments)]
    fn record_version_with(
        &mut self,
        uid: u64,
        path: &Path,
        bytes: Vec<u8>,
        kind: VersionKind,
        trigger: &str,
        document_id: Option<String>,
        origin: Option<VersionOrigin>,
        before: Option<DiskBefore>,
    ) {
        if self.version_store.is_none() {
            return;
        }
        let session = if kind == VersionKind::Autosave && trigger != "beforeRestore" { self.version_session(uid) } else { None };
        let mut details = self.version_details(uid, path, trigger, before, true);
        details.origin = origin;
        let private = self.history_doc(uid).is_some_and(|d| d.wb.remove_personal_info);
        let author = if private { String::new() } else { self.prefs.user_name.clone() };
        let write = VersionWrite {
            path: canonical_path(path),
            bytes,
            author,
            kind,
            saved_at: clock::now_iso(),
            session,
            document_id,
            details,
            cloud: self.cloud_roots.clone(),
        };
        if let Some(store) = self.version_store.as_mut() {
            store.save(write);
        }
    }

    /// Before `file.save` writes `path` for workbook `uid`: settle the document's identity and
    /// keep what is on disk.
    ///
    /// - A package gets a random document id, in its custom properties, at its first save (made
    ///   here or received from someone else). Save As (and Save to Cloud, Keep both) makes a new
    ///   file: it gets a new id and records which document and version it came from. A file that
    ///   carries the id of another file that still exists (a copy made outside the app, or two
    ///   copies open at once) gets a new id at its next save, also recording where it came
    ///   from. Other formats have no id and keep their history by path.
    /// - With "remove personal information" on, no id is written into the file (the history is
    ///   kept by path) and versions record no author.
    /// - Then the bytes on disk are kept if the history lacks them (an `original` or an
    ///   `external` version), synchronously, before the file is replaced.
    ///
    /// The returned [`PendingVersion`] carries the id to write into the file's bytes
    /// ([`PendingVersion::document_id`]) and what the version recorded after the save is.
    /// Nothing here can fail the save.
    pub(crate) fn prepare_version(&mut self, uid: u64, path: &Path, request: SaveRequest<'_>) -> Option<PendingVersion> {
        self.version_store.as_ref()?;
        let doc = self.history_doc(uid)?;
        let package = is_package(path);
        let own = doc.path.as_deref().is_some_and(|own| same_file_path(Path::new(own), path));
        let private = doc.wb.remove_personal_info;
        let has_file = doc.path.is_some();
        let current = doc.history.id.clone().filter(|id| meta::document_id_ok(id));
        let stamp_changed =
            own && doc.stamp.as_ref().is_some_and(|stamp| !matches!(stamp.clone().check(path), Ok(gridcraft_cloud::DiskState::Unchanged)));
        let mut kind = request.conflict_copy.then_some(VersionKind::ConflictCopy);
        let mut trigger = request.trigger.map(str::to_owned).unwrap_or_else(|| if request.autosave { "autosave".into() } else { "save".into() });
        let mut origin = None;
        let cloud = self.cloud_roots.clone();
        let canonical = canonical_path(path);
        let mut id = None;
        if package && !private {
            let previous = current.as_deref().and_then(|cur| self.version_store.as_ref().and_then(|s| s.newest(path, cur, &cloud)));
            if !own && has_file {
                // A new file from this document.
                origin = Some(VersionOrigin { document_id: current.clone(), version_id: previous.map(|(v, _)| v) });
                kind = kind.or(Some(VersionKind::SaveAs));
                trigger = if kind == Some(VersionKind::ConflictCopy) { "conflictCopy".into() } else { "saveAs".into() };
            } else if let Some(cur) = current.clone() {
                let checked = self.version_store.as_ref().is_some_and(|s| s.identity_checked.get(&uid) == Some(&canonical));
                if !checked
                    && let Some((version, Some(saved_at))) = previous
                    && !same_file_path(&saved_at, path)
                    && saved_at.is_file()
                    && meta::file_document_id(&saved_at).as_deref() == Some(cur.as_str())
                {
                    // Another file with this id is still there: this one is a copy.
                    origin = Some(VersionOrigin { document_id: Some(cur), version_id: Some(version) });
                    trigger = "copy".into();
                } else {
                    id = Some(cur);
                }
            }
            if id.is_none() {
                match meta::new_document_id() {
                    Ok(new) => id = Some(new),
                    Err(error) => {
                        if let Some(store) = self.version_store.as_mut() {
                            store.error = Some(error);
                        }
                    }
                }
            }
            if let Some(new) = &id {
                if let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
                    d.history.id = Some(new.clone());
                }
                if let Some(store) = self.version_store.as_mut() {
                    store.identity_checked.insert(uid, canonical.clone());
                }
            }
        } else if package && let Some(d) = self.docs.iter_mut().find(|d| d.uid == uid) {
            // Personal information removed: the file carries no id.
            d.history.id = None;
        }
        // Keep what is on disk, into the history this save goes to.
        let details = self.version_details(uid, path, "beforeSave", None, false);
        let mut before = None;
        let allowed = crate::io::file_access_allowed(&path.to_string_lossy());
        if let Some(store) = self.version_store.as_mut() {
            if stamp_changed || !own {
                store.verified.remove(&uid);
            }
            if allowed {
                match store.protect(uid, path, id.as_deref(), details, &cloud) {
                    Ok(found) => before = found,
                    Err(error) => {
                        log::warn!("version history could not keep {} before saving: {error}", path.display());
                        store.error = Some(error);
                    }
                }
            }
        }
        if before.is_none()
            && let Ok(file_meta) = std::fs::metadata(path)
        {
            // Known to hold the previous version; its hash comes from that version's record.
            before = Some(DiskBefore { size: file_meta.len(), modified: modified_secs(&file_meta).map(clock::iso_from_unix_secs), sha256: None });
        }
        Some(PendingVersion { kind, origin, trigger, before, document_id: id, path: canonical })
    }

    /// After a save wrote `bytes` to `path`, the document's own file: record the version.
    pub(crate) fn record_saved(&mut self, uid: u64, path: &Path, bytes: Vec<u8>, request: SaveRequest<'_>, pending: Option<PendingVersion>) {
        if self.version_store.is_none() {
            return;
        }
        let pending = pending.filter(|p| same_file_path(&p.path, path)).unwrap_or_default();
        let kind = pending.kind.unwrap_or(if request.autosave { VersionKind::Autosave } else { VersionKind::Save });
        let trigger =
            if pending.trigger.is_empty() { if request.autosave { "autosave".to_owned() } else { "save".to_owned() } } else { pending.trigger };
        let document_id =
            if pending.path.as_os_str().is_empty() { self.history_doc(uid).and_then(|d| history_id(d, Some(path))) } else { pending.document_id };
        if let Some(store) = self.version_store.as_mut() {
            // The file now holds the bytes the history is about to receive.
            store.verified.insert(uid, canonical_path(path));
        }
        self.record_version_with(uid, path, bytes, kind, &trigger, document_id, pending.origin, pending.before);
    }

    /// The history of a document kept by path follows it when it is renamed or moved by the host
    /// (there is no `file.rename` command yet), in order with the saves queued before it. A
    /// document with an id needs nothing: the id moves with the file, so a rename or move
    /// outside the app is followed too.
    pub fn versions_renamed(&mut self, uid: u64, from: PathBuf, to: &Path) {
        let by_path = self.version_document_id(uid).is_none();
        if let Some(store) = self.version_store.as_mut() {
            let verified = store.verified.get(&uid) == Some(&from);
            if by_path {
                store.renamed(from, to.to_path_buf());
            }
            store.verified.remove(&uid);
            if verified {
                store.verified.insert(uid, canonical_path(to));
            }
            if let Some(checked) = store.identity_checked.get_mut(&uid) {
                *checked = canonical_path(to);
            }
            store.current = None;
        }
    }

    /// Restore is about to replace workbook `uid`, which holds changes not yet written to its
    /// file: store them as a version first, so the state before the restore stays in the
    /// history (OneDrive likewise keeps the replaced current version in the list).
    pub(crate) fn keep_before_restore(&mut self, uid: u64, source: &Path) {
        if self.version_store.is_none() {
            return;
        }
        let Some(doc) = self.history_doc(uid) else { return };
        let id = history_id(doc, Some(source));
        let bytes = match crate::io::save_bytes(&doc.wb, &source.to_string_lossy()) {
            Ok(bytes) => with_document_id(bytes, id.as_deref()),
            Err(error) => {
                if let Some(store) = self.version_store.as_mut() {
                    store.error = Some(error.to_string());
                }
                return;
            }
        };
        // `Autosave` without a session: never joined into, nor joining, another version.
        self.record_version_with(uid, source, bytes, VersionKind::Autosave, "beforeRestore", id, None, None);
    }

    /// Restore replaced the workbook with version `version`: record the restore at once with
    /// those exact bytes, as OneDrive makes the restored version the newest one in the list
    /// right away (whether or not AutoSave writes the file afterwards).
    pub(crate) fn record_restore(&mut self, uid: u64, source: &Path, bytes: Vec<u8>, version: &str) {
        let id = self.version_document_id(uid);
        self.record_version_with(uid, source, bytes, VersionKind::Restore, &format!("restore:{version}"), id, None, None);
    }

    /// One version's metadata and exact bytes, for the history of the file at `path`.
    pub fn read_version(&mut self, uid: u64, path: &Path, id: &str) -> Result<(VersionInfo, Vec<u8>), String> {
        self.versions_tick();
        let document = self.version_document_id(uid);
        let store = self.version_store.as_ref().ok_or_else(|| "local version history is unavailable".to_string())?;
        store.read(path, document.as_deref(), id)
    }

    /// Delete the active workbook's history, or all history, in every history folder.
    pub fn delete_version_history(&mut self, scope: DeleteScope) -> Result<usize, String> {
        let (document, path) = match self.active() {
            Some(d) => (self.version_document_id(d.uid), d.path.clone().map(PathBuf::from)),
            None => (None, None),
        };
        let store = self.version_store.as_mut().ok_or_else(|| "local version history is unavailable".to_string())?;
        store.delete(scope, path.as_deref(), document.as_deref())
    }

    /// File › Options › Save › Version history changed: use the folders it names now.
    pub fn apply_version_folders(&mut self, existing: Relocation) -> Result<(), String> {
        let prefs = self.prefs.clone();
        if let Some(store) = self.version_store.as_mut() {
            let roots = version_roots(&store.default_root, &prefs);
            store.set_folders(roots, existing)?;
        }
        Ok(())
    }

    /// The history folders in use (empty where there is no history), those the last write could
    /// not reach, and whether history is being copied to new folders.
    pub fn version_folders(&self) -> (Vec<PathBuf>, Vec<PathBuf>, bool) {
        self.version_store.as_ref().map_or_else(|| (Vec::new(), Vec::new(), false), Versions::folders)
    }
}

/// `bytes` with the document id written into its custom properties (unchanged when there is no
/// id, or the package cannot be rewritten: the id is then written by the next save).
pub(crate) fn with_document_id(bytes: Vec<u8>, id: Option<&str>) -> Vec<u8> {
    match id {
        Some(id) => match gridcraft_xlsx::set_custom_property(&bytes, meta::DOC_ID_PROP, id) {
            Ok(patched) => patched,
            Err(error) => {
                log::warn!("the document id could not be written into the package: {error}");
                bytes
            }
        },
        None => bytes,
    }
}

/// `bytes` without the document id (a workbook that removes personal information).
pub(crate) fn without_document_id(bytes: Vec<u8>) -> Vec<u8> {
    match gridcraft_xlsx::remove_custom_property(&bytes, meta::DOC_ID_PROP) {
        Ok(Some(patched)) => patched,
        _ => bytes,
    }
}
