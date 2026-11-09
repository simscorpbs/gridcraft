//! Cloud AutoSave (iCloud Drive, Google Drive, OneDrive) through the folders their sync apps keep
//! (see `gridcraft-cloud` and `docs/cloud.md`).
//!
//! - `file.save` with `ifUnchanged` (what AutoSave sends) first checks that the workbook's file
//!   still holds what GridCraft last read or wrote. When another device or program changed it,
//!   nothing is written: the save reports the conflict and AutoSave waits for
//!   `file.resolveConflict` (keep both, keep this version, or take the other).
//! - `file.cloud` says where the workbook is kept and whether a conflict is waiting.
//! - `file.saveToCloud` saves a workbook straight into a provider's folder, as Excel offers to
//!   save to OneDrive when AutoSave is turned on for a new workbook.
//! - `file.autosave` is the AutoSave switch of the workbook, remembered for its file.
//!
//! Cloud folders are known only once the host looks for them (`Session::cloud_roots`); the
//! desktop app does, headless sessions and tests don't, so a script never writes into someone's
//! cloud by accident.

use std::path::Path;

use gridcraft_cloud::{CloudRoot, DiskState, Provider};
use serde_json::{Value as Json, json};

use super::{CommandSpec, Result, Session, bad, bool_param, cmd, has_doc, str_param};
use crate::autosave::AutoSaveWrite;
use crate::{DocState, EngineError, UiRequest};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "file.cloud", "Cloud Status", ["File", "Info"], None, "{} → {provider, name, root, path, conflict, notDownloaded, roots, autosave: {on, default, remembered, covers, blocked}}: where the active workbook is kept, the save conflict waiting (if any) and the cloud folders found on this computer", has_doc, status),
        cmd!(noundo "file.saveToCloud", "Save to Cloud", ["File", "Save As"], None, "{provider?: \"icloud\"|\"gdrive\"|\"onedrive\" (default: the first found, in that order), name?: string (file name without extension; default the workbook's name)} → {path, provider}", has_doc, save_to_cloud),
        cmd!(noundo "file.resolveConflict", "Resolve Save Conflict", ["File"], None, "{keep: \"both\" (save this version beside the other as \"Name (conflict copy).xlsx\" and keep editing that) | \"mine\" (overwrite the other version) | \"theirs\" (reopen the file as it is now; unsaved changes here are discarded)}", has_doc, resolve_conflict),
        cmd!(noundo "file.autosave", "AutoSave", ["File"], None, "{value?: bool} (turns AutoSave on or off for the active workbook and remembers it for its file; without `value` it toggles) → {value}. Turning it on for a workbook AutoSave cannot cover is an error that says why", has_doc, autosave),
    ]
}

/// The JSON for a provider's folder.
fn root_json(root: &CloudRoot) -> Json {
    json!({"provider": root.provider.id(), "name": root.provider.name(), "path": root.path, "account": root.account})
}

/// `file.cloud`: where the workbook is kept, the conflict waiting (if any) and the cloud folders
/// found on this computer.
fn status(s: &mut Session, _: &Json) -> Result<Json> {
    let d = s.doc()?;
    let path = d.path.as_deref().map(Path::new);
    let location = path.and_then(|p| gridcraft_cloud::locate(p, &s.cloud_roots));
    let not_downloaded = path.is_some_and(|p| gridcraft_cloud::icloud_placeholder(p).is_some());
    Ok(json!({
        "provider": location.as_ref().map(|l| l.provider.id()),
        "name": location.as_ref().map(|l| l.provider.name()),
        "root": location.as_ref().map(|l| &l.root),
        "path": d.path,
        "conflict": d.conflict.map(DiskState::id),
        "notDownloaded": not_downloaded,
        "roots": s.cloud_roots.iter().map(root_json).collect::<Vec<_>>(),
        // The switch for this workbook, the default for files without a choice, and the choice
        // remembered for this file (null: it follows the default).
        "autosave": {
            "on": d.autosave,
            "default": s.prefs.autosave_by_default,
            "remembered": s.autosave_remembered(d.uid),
            "covers": s.autosave_covers(d.uid),
            "blocked": d.autosave_block().map(|b| b.reason()),
        },
    }))
}

/// AutoSave's check before it writes the workbook's own file (`file.save` with `ifUnchanged`):
/// `Some(state)` when the file no longer holds what GridCraft last read or wrote, and the conflict
/// is recorded. A conflict already waiting stays until it is resolved.
pub(crate) fn conflict_before_save(s: &mut Session, path: &str) -> Result<Option<DiskState>> {
    let d = s.doc_mut()?;
    if let Some(conflict) = d.conflict {
        return Ok(Some(conflict));
    }
    if d.path.as_deref() != Some(path) {
        return Ok(None);
    }
    let Some(stamp) = d.stamp.as_mut() else { return Ok(None) };
    let state = stamp.check(Path::new(path)).map_err(|e| EngineError::Other(format!("{path}: {e}")))?;
    if state == DiskState::Unchanged {
        return Ok(None);
    }
    d.conflict = Some(state);
    Ok(Some(state))
}

/// `file.autosave`: the switch of the active workbook.
fn autosave(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let (uid, covers) = (d.uid, s.autosave_covers(d.uid));
    let on = bool_param(p, "value").unwrap_or(!covers);
    if on && let Some(block) = d.autosave_block() {
        return Err(bad("file.autosave", block.reason()));
    }
    s.set_autosave(uid, on);
    Ok(json!({"value": on}))
}

/// `file.saveToCloud`: save the workbook into a provider's documents folder under a name not
/// taken there, and keep it there (AutoSave can then cover it).
fn save_to_cloud(s: &mut Session, p: &Json) -> Result<Json> {
    let wanted = match str_param(p, "provider") {
        Some(id) => Some(Provider::from_id(id).ok_or_else(|| bad("file.saveToCloud", format!("unknown provider `{id}`; use icloud, gdrive or onedrive")))?),
        None => None,
    };
    let root = s.cloud_roots.iter().find(|r| wanted.is_none_or(|w| r.provider == w)).cloned().ok_or_else(|| {
        EngineError::Other(match wanted {
            Some(w) => format!("No {} folder was found on this computer. Install and sign in to its app, then try again.", w.name()),
            None => "No iCloud Drive, Google Drive or OneDrive folder was found on this computer.".to_string(),
        })
    })?;
    let stem = match str_param(p, "name") {
        Some(name) => name.to_string(),
        None => {
            let title = s.doc()?.display_title();
            Path::new(&title).file_stem().map_or_else(|| "Book".to_string(), |stem| stem.to_string_lossy().into_owned())
        }
    };
    let path = gridcraft_cloud::unique_path(&root.save_dir(), &stem, "xlsx")
        .ok_or_else(|| EngineError::Other(format!("Too many workbooks named {stem} in {}", root.provider.name())))?;
    let mut r = super::file::save(s, &json!({"path": path.to_string_lossy()}))?;
    if let Some(o) = r.as_object_mut() {
        o.insert("provider".into(), json!(root.provider.id()));
    }
    Ok(r)
}

/// `file.resolveConflict`: what to do once AutoSave found the file changed elsewhere.
fn resolve_conflict(s: &mut Session, p: &Json) -> Result<Json> {
    let d = s.doc()?;
    let (uid, conflict, path) = (d.uid, d.conflict, d.path.clone());
    let (Some(conflict), Some(path)) = (conflict, path) else {
        return Err(EngineError::Disabled("file.resolveConflict".into(), "no save conflict is waiting".into()));
    };
    let save = |s: &mut Session, path: &str| super::file::save(s, &json!({"path": path}));
    match str_param(p, "keep").ok_or_else(|| bad("file.resolveConflict", "missing `keep`"))? {
        "mine" => {
            set_conflict(s, uid, None);
            let r = save(s, &path);
            if r.is_err() {
                set_conflict(s, uid, Some(conflict));
            }
            r
        }
        "both" => {
            let copy = gridcraft_cloud::conflict_copy_path(Path::new(&path), "conflict copy")
                .ok_or_else(|| EngineError::Other(format!("Too many conflict copies of {path}")))?;
            // The session now edits the copy; the other version is left as it is.
            let r = super::file::save_with(s, &json!({"path": copy.to_string_lossy()}), true)?;
            set_conflict(s, uid, None);
            Ok(r)
        }
        "theirs" => take_theirs(s, uid, conflict, &path),
        other => Err(bad("file.resolveConflict", format!("`keep` must be both, mine or theirs, not `{other}`"))),
    }
}

fn set_conflict(s: &mut Session, uid: u64, conflict: Option<DiskState>) {
    if let Some(i) = s.document_index(uid)
        && let Some(d) = s.docs.get_mut(i)
    {
        d.conflict = conflict;
    }
}

/// Reopen the file as it is now, in place of the workbook (its unsaved changes are discarded).
fn take_theirs(s: &mut Session, uid: u64, conflict: DiskState, path: &str) -> Result<Json> {
    if conflict == DiskState::Missing {
        return Err(EngineError::Other(format!("{path} is no longer there to reopen")));
    }
    // Opened under its own name, the file would be "already open": let go of the path meanwhile.
    if let Some(i) = s.document_index(uid)
        && let Some(d) = s.docs.get_mut(i)
    {
        d.path = None;
    }
    match super::file::open(s, &json!({"path": path})) {
        Ok(r) => {
            if let Some(i) = s.document_index(uid) {
                s.close_document(i);
            }
            Ok(r)
        }
        Err(e) => {
            if let Some(i) = s.document_index(uid)
                && let Some(d) = s.docs.get_mut(i)
            {
                d.path = Some(path.to_string());
            }
            Err(e)
        }
    }
}

/// What the AutoSave attempt made before a workbook is closed came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flush {
    /// The changes are in the file: the workbook closes without a Save Changes question.
    Saved,
    /// The file was changed (or removed) elsewhere: the Save Conflict dialog was requested and
    /// the workbook stays open.
    Conflict(DiskState),
    /// AutoSave does not cover the workbook, or the write failed: the Save Changes question decides.
    Ask,
}

impl Session {
    /// Office-like closing: before a workbook with unsaved changes closes, AutoSave writes them
    /// (`file.save` with `ifUnchanged`) and the close goes on without a Save Changes question. If
    /// another device changed the file, a Save Conflict dialog is requested for the active window
    /// instead and the workbook stays open. With AutoSave off for the workbook, or a write that
    /// fails, the existing Save Changes question stays in charge, as it does when the host runs no
    /// AutoSave ([`Session::autosave_host`]). The workbook must be the
    /// active one (the dialog belongs to its window).
    pub fn autosave_before_discard(&mut self, uid: u64) -> Flush {
        let Some(d) = self.documents().iter().find(|d| d.uid == uid) else { return Flush::Ask };
        if !self.autosave_host || !d.is_dirty() || !d.autosave {
            return Flush::Ask;
        }
        let flush = match d.conflict {
            Some(conflict) if d.autosave_block().is_some_and(|b| matches!(b, crate::autosave::AutoSaveBlock::Conflict { .. })) => Flush::Conflict(conflict),
            _ => match self.autosave_write_for(uid, Some("close")) {
                Ok(AutoSaveWrite::Saved) => Flush::Saved,
                Ok(AutoSaveWrite::Conflict(conflict)) => Flush::Conflict(conflict),
                Ok(AutoSaveWrite::NotCovered) | Err(_) => Flush::Ask,
            },
        };
        if let Flush::Conflict(conflict) = flush {
            let title = self.documents().iter().find(|d| d.uid == uid).map(DocState::display_title).unwrap_or_default();
            self.ui_requests.push(UiRequest::Dialog("saveConflict".into(), json!({"title": title, "missing": conflict == DiskState::Missing})));
        }
        flush
    }
}
