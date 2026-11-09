//! File › Info › Version History (`file.versions`): the history GridCraft keeps for each saved
//! workbook, listed newest first, deleted, opened (its bytes and metadata, for a front end) and
//! restored. The store, the retention rules and the folders are in [`crate::versions`]; the
//! behaviour is described in `docs/cloud.md`.

#[cfg(target_arch = "wasm32")]
use serde_json::Value as Json;

use super::{CommandSpec, Result, Session, always, cmd};

#[cfg(not(target_arch = "wasm32"))]
use native::versions;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "file.versions",
        "Version History",
        ["File", "Info"],
        None,
        "{} → [{index, id, savedAt, author, size, stored, kind, storage, documentId, supersedes, details}] newest first (kind: save, autosave, restore, original, external, saveAs, conflictCopy; `stored` is the disk space the version added) | {delete: \"document\"|\"all\"} → {deleted} (this workbook's history, or every workbook's, in every history folder) | {restore: id} → {restored, savedAt}: replaces the workbook with that version in one Undo step; unsaved changes are kept first as an `autosave` version and the restored version is recorded at once as a `restore` version | {open: id} → {id, savedAt, author, size, kind, name, base64?}: the version's metadata and bytes (a .xlsx), without opening anything. The history needs the desktop app and a saved workbook",
        always,
        versions
    )]
}

/// The browser keeps no history.
#[cfg(target_arch = "wasm32")]
fn versions(_: &mut Session, _: &Json) -> Result<Json> {
    Err(crate::EngineError::Disabled("file.versions".into(), "the version history is available in the desktop app".into()))
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use serde_json::{Value as Json, json};

    use super::super::{bad, edit};
    use super::{Result, Session};
    use crate::EngineError;
    use crate::versions::{DeleteScope, VersionInfo, local_timestamp};

    /// The most bytes `{"open": id}` returns inside the command's result.
    const MAX_OPEN_BYTES: usize = 32 << 20;

    fn version_json(index: usize, info: &VersionInfo) -> Json {
        json!({
            "index": index,
            "id": info.id,
            "savedAt": info.saved_at,
            "author": info.author,
            "size": info.size,
            // The disk space the version added (a part shared by several versions counts once).
            "stored": info.stored.unwrap_or(info.size),
            "kind": info.kind.id(),
            "storage": info.storage,
            "documentId": info.document_id,
            "supersedes": info.supersedes,
            // Where the version comes from and what it holds: counts and hashes, never contents.
            "details": info.details,
        })
    }

    pub(super) fn versions(s: &mut Session, p: &Json) -> Result<Json> {
        let Some(object) = p.as_object() else {
            return Err(bad("file.versions", "parameters must be an object"));
        };
        for key in object.keys() {
            if !matches!(key.as_str(), "open" | "restore" | "delete") {
                return Err(bad("file.versions", format!("unknown parameter `{key}`")));
            }
        }
        if let Some(scope) = object.get("delete") {
            if object.len() > 1 {
                return Err(bad("file.versions", "`delete` goes alone"));
            }
            let scope = match scope.as_str() {
                Some("document") => DeleteScope::Document,
                Some("all") => DeleteScope::All,
                _ => return Err(bad("file.versions", "`delete` is \"document\" (this workbook's history) or \"all\"")),
            };
            if !s.has_versions() {
                return Err(EngineError::Disabled("file.versions".into(), "the version history is available in the desktop app".into()));
            }
            let deleted = s.delete_version_history(scope).map_err(EngineError::Other)?;
            return Ok(json!({"deleted": deleted}));
        }
        let open = object.get("open").filter(|v| !v.is_null());
        let restore = object.get("restore").filter(|v| !v.is_null());
        match (open, restore) {
            (Some(_), Some(_)) => Err(bad("file.versions", "choose one of open or restore")),
            (Some(id), None) => open_version(s, id.as_str().ok_or_else(|| bad("file.versions", "`open` must be a version id"))?),
            (None, Some(id)) => restore_version(s, id.as_str().ok_or_else(|| bad("file.versions", "`restore` must be a version id"))?),
            (None, None) => {
                let list = s.version_infos();
                Ok(Json::Array(list.iter().enumerate().map(|(i, info)| version_json(i, info)).collect()))
            }
        }
    }

    /// The active workbook's id, file, and the check that the history can be used.
    fn source(s: &Session) -> Result<(u64, std::path::PathBuf)> {
        let d = s.doc()?;
        if !s.has_versions() {
            return Err(EngineError::Disabled("file.versions".into(), "the version history is available in the desktop app".into()));
        }
        let path =
            d.path.as_deref().ok_or_else(|| EngineError::Disabled("file.versions".into(), "the version history needs a saved workbook".into()))?;
        Ok((d.uid, std::path::PathBuf::from(path)))
    }

    /// `{"open": id}`: a version's metadata and bytes. Nothing in the session changes.
    fn open_version(s: &mut Session, id: &str) -> Result<Json> {
        let (uid, path) = source(s)?;
        let (info, bytes) = s.read_version(uid, &path, id).map_err(EngineError::Other)?;
        let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Book".into());
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "xlsx".into());
        let mut r = json!({
            "id": info.id,
            "savedAt": info.saved_at,
            "author": info.author,
            "size": info.size,
            "kind": info.kind.id(),
            // What a front end titles the read-only copy: `Name (version <local date time>)`.
            "name": format!("{stem} (version {}).{ext}", local_timestamp(&info.saved_at)),
            "bytes": bytes.len(),
        });
        if bytes.len() <= MAX_OPEN_BYTES {
            r["base64"] = json!(crate::io::base64_encode(&bytes));
        } else {
            r["tooLarge"] = json!(true);
        }
        Ok(r)
    }

    /// `{"restore": id}`: the version replaces the workbook, in one Undo step.
    fn restore_version(s: &mut Session, id: &str) -> Result<Json> {
        let (uid, path) = source(s)?;
        let (info, bytes) = s.read_version(uid, &path, id).map_err(EngineError::Other)?;
        let name = path.to_string_lossy().into_owned();
        let (mut restored, _) = crate::io::open_bytes_with(&name, &bytes, None, false)?;
        let dirty = s.doc()?.is_dirty();
        // The state being replaced stays in the history: unsaved changes are stored first.
        if dirty {
            s.keep_before_restore(uid, &path);
        }
        edit(s, |cx| {
            restored.source_dir = cx.wb.source_dir.clone();
            restored.source_path = cx.wb.source_path.clone();
            cx.wb = restored;
            cx.structural = true;
            Ok(())
        })?;
        // The restore is a version at once, with the restored bytes.
        s.record_restore(uid, &path, bytes, &info.id);
        Ok(json!({"restored": info.id, "savedAt": info.saved_at}))
    }
}
