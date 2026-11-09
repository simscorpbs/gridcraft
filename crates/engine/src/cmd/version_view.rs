//! Previous versions (File › Info › Version History): a version opens as a separate, read-only
//! workbook beside the current one (`file.openVersion`), can be compared with it
//! (`file.compareVersion`), restored (`file.restoreVersion`, which is `file.versions` with
//! `restore` run from the version) or saved as a copy (`file.saveVersionCopy`), and closes back
//! to the current workbook (`file.closeVersion`). See [`crate::version_view`] and
//! `docs/cloud.md`.

use serde_json::{Value as Json, json};

use super::{CommandSpec, Result, Session, bad, bool_param, cmd, ok, str_param};
use crate::{EngineError, UiRequest};

/// Enabled while the active workbook is a previous version.
fn is_version(s: &Session) -> std::result::Result<(), String> {
    if s.previous_version().is_some() { Ok(()) } else { Err("no previous version is open".into()) }
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "file.openVersion", "Open Version", ["File", "Info", "Version History"], None, "{id} → {opened, doc, name, savedAt, author}: opens that version of the active workbook's history as a separate, read-only workbook beside it (a previous version shown from the Version History pane; edits are refused). AutoSave writes the workbook's pending changes first. A save conflict found by that write is reported in `conflict` and nothing opens. The version already open is brought forward. Needs the desktop app and a saved workbook", crate::cmd::always, open_version),
        cmd!(query "file.closeVersion", "Close Version", ["File", "Info", "Version History"], None, "{doc?} → {closed, doc}: closes the previous version (the active one, or workbook `doc`) and brings back the workbook it came from, exactly as it was", is_version, close_version),
        cmd!(query "file.compareVersion", "Compare Version", ["File", "Info", "Version History"], None, "{on?: bool} (default: toggle) → {compared, on, differences, sheets: [{sheet, cells}], onlyInVersion, onlyInCurrent, truncated}: shows the previous version with the cells that differ from the current workbook highlighted (a cell whose value or formula changed, or that the current workbook has and the version lacks); the comparison takes the version's place and stays read only, `on: false` shows the version again", is_version, compare_version),
        cmd!(query "file.restoreVersion", "Restore Version", ["File", "Info", "Version History"], None, "{} → file.versions {restore} result: returns to the workbook the previous version came from, makes the version its content in one Undo step (see file.versions) and closes the version", is_version, restore_version),
        cmd!(query "file.saveVersionCopy", "Save a Copy of the Version", ["File", "Info", "Version History"], None, "{path} → {saved, path, bytes, closed, doc}: saves the previous version as a new file (a new document id, recording the version it came from in the history) and goes back to the workbook it came from. Without `path` the interface asks where. The workbook the version came from is never overwritten", is_version, save_copy),
    ]
}

#[cfg(target_arch = "wasm32")]
fn open_version(_: &mut Session, _: &Json) -> Result<Json> {
    Err(EngineError::Disabled("file.openVersion".into(), "the version history is available in the desktop app".into()))
}

#[cfg(not(target_arch = "wasm32"))]
fn open_version(s: &mut Session, p: &Json) -> Result<Json> {
    use crate::autosave::AutoSaveWrite;
    use crate::version_view::{VersionView, title_for};
    use crate::versions::DocHistory;
    use crate::{DocState, Trust};

    let id = str_param(p, "id").ok_or_else(|| bad("file.openVersion", "missing `id`"))?;
    if !s.has_versions() {
        return Err(EngineError::Disabled("file.openVersion".into(), "the version history is available in the desktop app".into()));
    }
    let (uid, path) =
        s.history_source().ok_or_else(|| EngineError::Disabled("file.openVersion".into(), "the version history needs a saved workbook".into()))?;
    if let Some(i) = s.documents().iter().position(|d| d.version.as_ref().is_some_and(|v| v.source_uid == uid && v.id == id)) {
        s.set_active(i);
        let d = s.doc()?;
        return Ok(json!({"opened": id, "alreadyOpen": true, "doc": d.uid, "name": d.display_title()}));
    }
    // AutoSave writes the workbook's pending changes first, so that state is in the history too.
    // The workbook is the active one while it does (a conflict dialog belongs to it).
    let pending = s.documents().iter().find(|d| d.uid == uid).is_some_and(|d| d.is_dirty() && d.autosave);
    if s.autosave_host && pending {
        if let Some(i) = s.document_index(uid) {
            s.set_active(i);
        }
        // A write that fails or is not covered leaves the changes in the workbook, untouched.
        if let Ok(AutoSaveWrite::Conflict(conflict)) = s.autosave_write_for(uid, Some("open")) {
            let title = s.documents().iter().find(|d| d.uid == uid).map(DocState::display_title).unwrap_or_default();
            s.ui_requests.push(UiRequest::Dialog(
                "saveConflict".into(),
                json!({"title": title, "missing": conflict == crate::autosave::DiskState::Missing}),
            ));
            return Ok(json!({"opened": false, "conflict": conflict.id()}));
        }
    }
    let (info, bytes) = s.read_version(uid, &path, id).map_err(EngineError::Other)?;
    let name = title_for(&path, &info.saved_at);
    let (mut wb, _) = crate::io::open_bytes_with(&name, &bytes, None, false)?;
    if let Some(source) = s.documents().iter().find(|d| d.uid == uid) {
        wb.source_dir.clone_from(&source.wb.source_dir);
        wb.source_path.clone_from(&source.wb.source_path);
    }
    let mut doc = DocState::new(wb, None, name.clone(), Trust::FromFile);
    doc.history = DocHistory::opened(&bytes);
    doc.version = Some(VersionView {
        source_uid: uid,
        source_path: path.to_string_lossy().into_owned(),
        id: info.id.clone(),
        saved_at: info.saved_at.clone(),
        author: info.author.clone(),
        compared: false,
        plain: None,
    });
    let at = s.add_document(doc);
    if let Some(d) = s.docs.get_mut(at) {
        d.autosave = false;
    }
    Ok(json!({"opened": info.id, "doc": s.doc()?.uid, "name": name, "savedAt": info.saved_at, "author": info.author}))
}

fn close_version(s: &mut Session, p: &Json) -> Result<Json> {
    let uid = match p.get("doc").filter(|v| !v.is_null()) {
        Some(v) => v.as_u64().ok_or_else(|| bad("file.closeVersion", "`doc` must be a workbook id"))?,
        None => s.doc()?.uid,
    };
    let id = s
        .documents()
        .iter()
        .find(|d| d.uid == uid)
        .and_then(|d| d.version.as_ref())
        .map(|v| v.id.clone())
        .ok_or_else(|| EngineError::Disabled("file.closeVersion".into(), "no previous version is open".into()))?;
    let back = s.close_previous_version(uid);
    Ok(json!({"closed": id, "doc": back}))
}

/// How many highlighted cells a comparison keeps; past it the rest are counted but not shown.
const MAX_MARKED: usize = 100_000;

/// The cell fill of a difference.
pub(crate) fn marker() -> gridcraft_model::Fill {
    gridcraft_model::Fill::solid(gridcraft_model::Color::rgb(0xFF, 0xE0, 0x82))
}

/// `version` with the cells that differ from `current` highlighted, and a summary.
fn highlight(version: &gridcraft_model::Workbook, current: &gridcraft_model::Workbook) -> (gridcraft_model::Workbook, Json) {
    use std::collections::HashMap;
    use std::sync::Arc;

    let mut wb = version.clone();
    let mut total = 0usize;
    let mut sheets = Vec::new();
    let mut only_version = Vec::new();
    let mut marked: HashMap<gridcraft_model::StyleId, gridcraft_model::StyleId> = HashMap::new();
    for si in 0..version.sheets.len() {
        let Some(old) = version.sheets.get(si) else { continue };
        let Some(new) = current.sheet_index(&old.name).and_then(|i| current.sheets.get(i)) else {
            only_version.push(old.name.clone());
            continue;
        };
        let mut at: Vec<gridcraft_core::CellRef> = Vec::new();
        for (cell_ref, cell) in old.cells.iter() {
            let differs = match new.cells.get(cell_ref) {
                Some(other) => {
                    cell.formula != other.formula || cell.py != other.py || (cell.formula.is_none() && cell.py.is_none() && cell.value != other.value)
                }
                None => cell.has_content(),
            };
            if differs {
                at.push(cell_ref);
            }
        }
        let seen: std::collections::HashSet<gridcraft_core::CellRef> = at.iter().copied().collect();
        for (cell_ref, cell) in new.cells.iter() {
            if cell.has_content() && old.cells.get(cell_ref).is_none_or(|c| !c.has_content()) && !seen.contains(&cell_ref) {
                at.push(cell_ref);
            }
        }
        if at.is_empty() {
            continue;
        }
        sheets.push(json!({"sheet": old.name, "cells": at.len()}));
        for cell_ref in at {
            total += 1;
            if total > MAX_MARKED {
                continue;
            }
            let style = old.style_id(cell_ref);
            let styled = *marked.entry(style).or_insert_with(|| {
                let mut s = wb.styles.get(style).clone();
                s.fill = marker();
                wb.styles.intern(s)
            });
            if let Some(sheet) = wb.sheets.get_mut(si) {
                Arc::make_mut(sheet).set_style(cell_ref, styled);
            }
        }
    }
    let only_current: Vec<String> = current.sheets.iter().filter(|s| version.sheet_index(&s.name).is_none()).map(|s| s.name.clone()).collect();
    let summary = json!({
        "differences": total,
        "sheets": sheets,
        "onlyInVersion": only_version,
        "onlyInCurrent": only_current,
        "truncated": total > MAX_MARKED,
    });
    (wb, summary)
}

fn compare_version(s: &mut Session, p: &Json) -> Result<Json> {
    let (uid, id, source, compared) = {
        let d = s.doc()?;
        let v = d.version.as_ref().ok_or_else(|| EngineError::Disabled("file.compareVersion".into(), "no previous version is open".into()))?;
        (d.uid, v.id.clone(), v.source_uid, v.compared)
    };
    let on = bool_param(p, "on").unwrap_or(!compared);
    let i = s.document_index(uid).ok_or(EngineError::NoDocument)?;
    if on == compared {
        return Ok(json!({"compared": id, "on": on}));
    }
    if on {
        let current = s
            .documents()
            .iter()
            .find(|d| d.uid == source)
            .map(|d| d.wb.clone())
            .ok_or_else(|| EngineError::Other("the workbook this version came from is closed".into()))?;
        let d = s.docs.get_mut(i).ok_or(EngineError::NoDocument)?;
        let plain = d.wb.clone();
        let (shown, mut summary) = highlight(&plain, &current);
        d.wb = std::sync::Arc::new(shown);
        d.saved = d.wb.clone();
        d.revision += 1;
        if let Some(v) = d.version.as_mut() {
            v.plain = Some(plain);
            v.compared = true;
        }
        if let Some(o) = summary.as_object_mut() {
            o.insert("compared".into(), json!(id));
            o.insert("on".into(), json!(true));
        }
        Ok(summary)
    } else {
        show_plain(s, i);
        Ok(json!({"compared": id, "on": false}))
    }
}

/// Put the version as saved back in the place of its comparison.
fn show_plain(s: &mut Session, i: usize) {
    if let Some(d) = s.docs.get_mut(i)
        && let Some(v) = d.version.as_mut()
        && let Some(plain) = v.plain.take()
    {
        v.compared = false;
        d.wb = plain;
        d.saved = d.wb.clone();
        d.revision += 1;
    }
}

fn restore_version(s: &mut Session, _: &Json) -> Result<Json> {
    let (uid, id, source) = {
        let d = s.doc()?;
        let v = d.version.as_ref().ok_or_else(|| EngineError::Disabled("file.restoreVersion".into(), "no previous version is open".into()))?;
        (d.uid, v.id.clone(), v.source_uid)
    };
    let back = s.document_index(source).ok_or_else(|| EngineError::Other("the workbook this version came from is closed".into()))?;
    // Back to the current workbook first: the restore replaces its content in one Undo step.
    s.set_active(back);
    match s.execute("file.versions", json!({"restore": id})) {
        Ok(result) => {
            s.close_previous_version(uid);
            Ok(result)
        }
        Err(e) => {
            if let Some(i) = s.document_index(uid) {
                s.set_active(i);
            }
            Err(e)
        }
    }
}

fn save_copy(s: &mut Session, p: &Json) -> Result<Json> {
    let Some(path) = str_param(p, "path").map(str::to_string) else {
        s.ui_requests.push(UiRequest::Dialog("saveVersionCopy".into(), json!({})));
        return ok();
    };
    let (uid, id, source_path) = {
        let d = s.doc()?;
        let v = d.version.as_ref().ok_or_else(|| EngineError::Disabled("file.saveVersionCopy".into(), "no previous version is open".into()))?;
        (d.uid, v.id.clone(), v.source_path.clone())
    };
    let key = |p: &str| crate::autosave::file_key(std::path::Path::new(p));
    if key(&path).is_some() && key(&path) == key(&source_path) {
        return Err(bad("file.saveVersionCopy", "that is the workbook this version came from: save the copy under another name, or use Restore"));
    }
    let i = s.document_index(uid).ok_or(EngineError::NoDocument)?;
    // The version as saved, not its comparison; the copy follows the default AutoSave setting.
    show_plain(s, i);
    let default_autosave = s.prefs.autosave_by_default;
    if let Some(d) = s.docs.get_mut(i) {
        d.autosave = default_autosave;
    }
    match super::file::save(s, &json!({"path": path})) {
        Ok(mut result) => {
            let back = s.close_previous_version(uid);
            if let Some(o) = result.as_object_mut() {
                o.insert("closed".into(), json!(id));
                o.insert("doc".into(), json!(back));
            }
            Ok(result)
        }
        Err(e) => {
            if let Some(d) = s.docs.get_mut(i) {
                d.autosave = false;
            }
            Err(e)
        }
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests;
