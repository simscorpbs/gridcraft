//! A previous version opened from File › Info › Version History, shown the way Excel shows one:
//! as a separate, read-only workbook beside the current one. It can be compared with the current
//! workbook, restored or saved as a copy, and closing it returns to the current workbook, which
//! was never touched (its unsaved changes, undo history and AutoSave stamp stay where they were).
//!
//! GridCraft keeps several workbooks open, so the version is one more [`crate::DocState`] with a
//! [`VersionView`]. The commands that open and leave it are in `cmd::version_view`.

use std::sync::Arc;

use gridcraft_model::Workbook;

use crate::cmd::CommandSpec;
use crate::{DocState, Session};

/// Why an edit is refused while a previous version is shown (the status line shows it).
pub const READ_ONLY: &str = "This is a previous version. Restore it or save a copy to make changes.";

/// The previous version a [`DocState`] shows.
#[derive(Clone, Debug)]
pub struct VersionView {
    /// The workbook the version belongs to ([`DocState::uid`]). It may have been closed since.
    pub source_uid: u64,
    /// The file of that workbook, whose history the version is in.
    pub source_path: String,
    /// The version's id in that history.
    pub id: String,
    /// When it was saved (UTC, `YYYY-MM-DDTHH:MM:SSZ`).
    pub saved_at: String,
    pub author: String,
    /// Whether the cells that differ from the current workbook are highlighted (Compare).
    pub compared: bool,
    /// The version as saved, kept while the comparison is shown in its place.
    pub plain: Option<Arc<Workbook>>,
}

impl DocState {
    /// Whether this workbook is a previous version, which cannot be changed.
    pub fn is_previous_version(&self) -> bool {
        self.version.is_some()
    }
}

/// What still works while a previous version is shown even though the command would change a
/// workbook: the version commands themselves (the history listing reads, Restore and Save a copy
/// leave the version first) and the view.
fn allowed(id: &str) -> bool {
    id.starts_with("view.")
        || matches!(
            id,
            "file.versions" | "file.openVersion" | "file.closeVersion" | "file.compareVersion" | "file.restoreVersion" | "file.saveVersionCopy"
        )
}

/// Commands that change nothing in the undo history but would still change the version or write
/// its (nonexistent) file: Undo and Redo have no edits to take back, and Save would write a file
/// the version does not have (Save a copy is the way).
const REFUSED_ALWAYS: &[&str] = &[
    "edit.undo",
    "edit.redo",
    "edit.cut",
    "edit.beginEdit",
    "file.save",
    "file.saveAs",
    "file.saveToCloud",
    "file.resolveConflict",
    "file.rename",
    "macro.run",
    "automate.runScript",
];

/// The reason command `spec` is refused on the active workbook, when that is a previous version.
/// `None` for anything that works (queries, selecting, copying, the view, file commands that do
/// not write this workbook).
pub(crate) fn refusal(s: &Session, spec: &CommandSpec) -> Option<&'static str> {
    if !s.active().is_some_and(DocState::is_previous_version) || allowed(spec.id) {
        return None;
    }
    (spec.undoable || REFUSED_ALWAYS.contains(&spec.id)).then_some(READ_ONLY)
}

/// A local date and time for the title of a version: the saved time on the computer's clock.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn title_for(path: &std::path::Path, saved_at: &str) -> String {
    let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "Book".into());
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "xlsx".into());
    format!("{stem} (version {}).{ext}", crate::versions::local_timestamp(saved_at))
}

impl Session {
    /// The previous version the active workbook shows, if it is one.
    pub fn previous_version(&self) -> Option<&VersionView> {
        self.active().and_then(|d| d.version.as_ref())
    }

    /// The open previous versions, as (workbook uid, view).
    pub fn previous_versions(&self) -> impl Iterator<Item = (u64, &VersionView)> + '_ {
        self.documents().iter().filter_map(|d| d.version.as_ref().map(|v| (d.uid, v)))
    }

    /// Close the previous version `uid` and, when it was the active workbook, bring back the
    /// workbook it came from if that is still open. Returns that workbook's uid.
    pub fn close_previous_version(&mut self, uid: u64) -> Option<u64> {
        let i = self.document_index(uid)?;
        let source = self.documents().get(i).and_then(|d| d.version.as_ref()).map(|v| v.source_uid)?;
        let was_active = self.active_index() == i;
        self.close_document(i);
        let back = self.document_index(source);
        if was_active && let Some(b) = back {
            self.set_active(b);
        }
        back.map(|_| source)
    }

    /// Close every previous version, bringing the current workbooks back (the interface does it
    /// before quitting, so a version shown never hides a workbook's unsaved work).
    pub fn close_previous_versions(&mut self) {
        let open: Vec<u64> = self.previous_versions().map(|(uid, _)| uid).collect();
        for uid in open {
            self.close_previous_version(uid);
        }
    }
}

/// The last line of defence: whatever command ran, a previous version comes out as it went in.
/// The commands that change a workbook are refused before they run ([`refusal`]); this puts back
/// the workbook of one that got past (a macro step, a command not yet known to change things).
pub(crate) fn keep_read_only(
    s: &mut Session,
    id: &str,
    before: Option<(u64, &Arc<Workbook>)>,
    result: crate::Result<serde_json::Value>,
) -> crate::Result<serde_json::Value> {
    let Some((uid, wb)) = before else { return result };
    if result.is_err() || allowed(id) {
        return result;
    }
    let Some(i) = s.document_index(uid) else { return result };
    let Some(d) = s.docs.get_mut(i) else { return result };
    if d.version.is_none() || Arc::ptr_eq(&d.wb, wb) {
        return result;
    }
    d.wb = wb.clone();
    d.calc.rebuild(&d.wb);
    Err(crate::EngineError::Disabled(id.to_string(), READ_ONLY.to_string()))
}
