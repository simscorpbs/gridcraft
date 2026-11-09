//! File menu: new, open, save, close, export, properties, options.

use serde_json::{Value as Json, json};

use super::{CommandSpec, Result, Session, always, bad, bool_param, cmd, edit, has_doc, ok, str_param, target_sheet, u32_param};
use crate::{DocState, EngineError};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "file.new", "New Workbook", ["File"], Some("Cmd+N"), "{sample?: \"budget\"|\"sales\"|\"grades\"}", always, new_workbook),
        cmd!(noundo "file.open", "Open…", ["File"], Some("Cmd+O"), "{path} | {name, base64}, plus {password} for a password-protected workbook and {csvAsText?: bool} to keep every csv/tsv/txt field as text, with no number or date conversion (xlsx, xlsm, xls import, xlsb and ods data import, csv, tsv, txt, json)", always, open),
        cmd!(noundo "file.save", "Save", ["File"], Some("Cmd+S"), "{path?, ifUnchanged?: bool, trigger?: string} (xlsx by default; .csv/.tsv/.json/.html/.pdf by extension; AutoSave sends `ifUnchanged`: the workbook's own file is written only if no other device or program changed it since it was read or written here, else nothing is written and the result has `conflict` (changed or missing), see file.resolveConflict; `trigger` says what asked for the save, recorded in the version history, e.g. `close`) → {saved, path, bytes}. In the desktop app the workbook's own file also gets a version in its history (file.versions)", has_doc, save),
        cmd!(noundo "file.rename", "Rename", ["File", "Info"], None, "{name} → {renamed, path, from}: renames the active workbook's file on disk, in its folder (the title-bar file menu). `name` is the new file name; without the file's extension it is added. The file keeps its version history, its AutoSave switch and its place in the list. Refuses a name that already exists, one with a folder or characters Windows does not allow, and a workbook with no file", has_doc, rename),
        cmd!(noundo "file.saveAs", "Save As…", ["File"], Some("Cmd+Shift+S"), "{path} (.xlsx, .csv, .tsv, .json, .html, or .pdf: a PDF of the active sheet)", has_doc, save_as),
        cmd!(query "file.saveBytes", "Encode Workbook", [], None, "{format?: xlsx|csv|tsv|json|html} → {base64}", has_doc, save_bytes),
        cmd!(noundo "file.close", "Close", ["File"], Some("Cmd+W"), "{force?: bool, document?: bool} (closes the active window, or with `document` the workbook and all its windows; the workbook closes with its last window)", has_doc, close),
        cmd!(noundo "file.exportCsv", "Export as CSV", ["File", "Export"], None, "{path, sheet?, values?: bool} → {path, bytes}: writes the cells as displayed, like Excel's Save As CSV; `values: true` writes raw values instead (full-precision numbers, ISO 8601 dates and times, TRUE/FALSE)", has_doc, |s, p| export(s, p, "csv")),
        cmd!(noundo "file.exportHtml", "Save as Web Page", ["File", "Export"], None, "{path}", has_doc, |s, p| export(s, p, "html")),
        cmd!("file.properties", "Properties", ["File"], None, "{title?, subject?, author?, company?, keywords?, description?}", has_doc, properties),
        cmd!(
            query "file.options",
            "Options",
            ["File"],
            None,
            "{autosaveByDefault?: bool, versionsFolder?: string, versionsMirror?: bool, versionsExisting?: \"copy\"|\"move\"|\"leave\", deviceLabel?: string} → {calculation: {mode, iterative, maxIterations, maxChange, precisionAsDisplayed}, autosaveByDefault, versionsFolder, versionsMirror, versionsFolders, versionsUnavailable, versionsRelocating, deviceLabel, deviceLabelDefault} for the active workbook (the interface opens File › Options; with `autosaveByDefault` it only sets whether AutoSave is on for files with no choice of their own; `versionsFolder` (an existing folder, or `default` for the settings folder alone) keeps the version history there instead, `versionsMirror` also keeps it in the settings folder, `versionsExisting` says whether the history already kept is copied (default), moved or left when the folders change, and `deviceLabel` (or `auto`) is the device name versions record; change other settings with their own commands, e.g. formulas.calculationOptions, view.theme, app.language.set)",
            always,
            options
        ),
        cmd!(noundo "window.activate", "Switch Window", ["Window"], None, "{window} | {doc} | {index} (a window id, a workbook id, or the workbook's position in the list)", has_doc, activate_window),
        cmd!(noundo "window.close", "Close Window", ["Window"], None, "{window?, doc?, force?: bool} → {closed, document}: closes a window (the active one by default); its workbook closes with its last window, asking first when it has unsaved changes unless `force`", has_doc, close_window),
        cmd!(query "window.list", "Windows", ["Window"], None, "{} → [{window, doc, index, title, dirty, active, hidden}]", always, |s, _| Ok(s.window_list())),
    ]
}

/// New, Open and Close while a previous version is shown first go back to the current workbook,
/// so what follows (a new workbook, the Save Changes question) starts from it.
fn leave_previous_version(s: &mut Session) -> Result<()> {
    if let Some(uid) = s.active().filter(|d| d.is_previous_version()).map(|d| d.uid) {
        s.close_previous_version(uid);
    }
    Ok(())
}

fn new_workbook(s: &mut Session, p: &Json) -> Result<Json> {
    leave_previous_version(s)?;
    let i = match str_param(p, "sample") {
        Some(name) => {
            let wb = crate::sample::build(name).ok_or_else(|| bad("file.new", format!("unknown sample `{name}`")))?;
            let title = crate::sample::title(name);
            s.add_document(DocState::new(wb, None, title, crate::Trust::Local))
        }
        None => s.new_workbook(),
    };
    Ok(json!({"index": i, "title": s.doc()?.display_title()}))
}

pub(super) fn open(s: &mut Session, p: &Json) -> Result<Json> {
    leave_previous_version(s)?;
    let (name, bytes) = if let Some(b) = str_param(p, "base64") {
        (str_param(p, "name").unwrap_or("Book.xlsx").to_string(), crate::io::base64_decode(b).ok_or_else(|| bad("file.open", "invalid base64"))?)
    } else if let Some(path) = str_param(p, "path") {
        // Already open? Switch to it.
        if let Some(i) = s.documents().iter().position(|d| d.path.as_deref() == Some(path)) {
            s.set_active(i);
            return Ok(json!({"index": i, "alreadyOpen": true}));
        }
        (path.to_string(), crate::io::read_file(path)?)
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("open".into(), json!({})));
        return ok();
    };
    let encrypted = gridcraft_xlsx::sniff(&bytes) == gridcraft_xlsx::Format::Encrypted;
    let (mut wb, warnings) = crate::io::open_bytes_with(&name, &bytes, str_param(p, "password"), bool_param(p, "csvAsText").unwrap_or(false))?;
    wb.source_dir = str_param(p, "path").and_then(crate::io::source_dir);
    // What `CELL("filename")` reports (#1489). A workbook opened from bytes knows its name but
    // never a folder, so it reports `[name]Sheet` with none; an import reports the file it came
    // from, as Excel does, even though Save must still ask where to put the .xlsx.
    wb.source_path = match str_param(p, "path") {
        Some(path) => crate::io::source_path(path),
        None => Some(std::path::PathBuf::from(&name)),
    };
    let imported =
        matches!(crate::io::FileKind::from_path(&name), Some(crate::io::FileKind::Ods | crate::io::FileKind::Xlsb | crate::io::FileKind::Xls))
            || matches!(gridcraft_xlsx::sniff(&bytes), gridcraft_xlsx::Format::Ods | gridcraft_xlsx::Format::Xlsb | gridcraft_xlsx::Format::Xls);
    let title = if imported {
        format!("{} (imported).xlsx", std::path::Path::new(&name).file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("Book"))
    } else {
        std::path::Path::new(&name).file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("Book").to_string()
    };
    // An import has no save target: ordinary Save must ask for a new destination. Neither does a
    // workbook decrypted with its password, so Save never replaces the encrypted original with
    // an unprotected copy.
    let path = str_param(p, "path").filter(|_| !imported && !encrypted).map(str::to_string);
    // Replace an untouched blank Book1 like Excel does.
    if s.documents().len() == 1
        && s.active().is_some_and(|d| d.path.is_none() && !d.is_dirty() && d.undo.is_empty() && d.wb.sheets.iter().all(|sh| sh.cells.is_empty()))
    {
        s.close_document(0);
    }
    // Someone else may have made the file: its active content asks before it runs.
    let stamp = path.as_deref().and_then(|p| gridcraft_cloud::DiskStamp::of_bytes(std::path::Path::new(p), &bytes).ok());
    let mut doc = DocState::new(wb, path, title, crate::Trust::FromFile);
    // The id the file carries, if it was saved here before: its history is kept under it.
    #[cfg(not(target_arch = "wasm32"))]
    {
        doc.history = crate::versions::DocHistory::opened(&bytes);
    }
    // What AutoSave checks before it writes this file: that it still holds what was just read.
    doc.stamp = stamp;
    if crate::io::FileKind::from_path(&name).is_none() && gridcraft_xlsx::sniff(&bytes) == gridcraft_xlsx::Format::Xlsx {
        doc.content_kind = Some(crate::io::FileKind::Xlsx);
    }
    let i = s.add_document(doc);
    // The decrypted-workbook notice is for the info bar, not a message box.
    let shown: Vec<&str> = warnings.iter().map(String::as_str).filter(|w| *w != crate::io::DECRYPTED_NOTICE).collect();
    if !shown.is_empty() {
        let mut message = shown.iter().take(5).copied().collect::<Vec<_>>().join("\n\n");
        if shown.len() > 5 {
            message.push_str(&format!("\n\n{} additional import warnings.", shown.len() - 5));
        }
        s.ui_requests.push(crate::UiRequest::Message(message));
    }
    let d = s.doc()?;
    Ok(
        json!({"index": i, "title": d.display_title(), "sheets": d.wb.sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>(), "warnings": warnings}),
    )
}

pub(super) fn save(s: &mut Session, p: &Json) -> Result<Json> {
    save_with(s, p, false)
}

/// [`save`]; with `conflict_copy` the file is the copy Keep both makes after a save conflict,
/// which the version history records as such.
pub(super) fn save_with(s: &mut Session, p: &Json, conflict_copy: bool) -> Result<Json> {
    let path = match str_param(p, "path").map(str::to_string).or_else(|| s.active().and_then(|d| d.path.clone())) {
        Some(p) => p,
        None => {
            s.ui_requests.push(crate::UiRequest::Dialog("saveAs".into(), json!({})));
            return ok();
        }
    };
    // What asked for the save, recorded in the version history: a short word such as `close`.
    match p.get("trigger") {
        None | Some(Json::Null) => {}
        Some(Json::String(t)) if !t.is_empty() && t.len() <= 32 && t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') => {}
        Some(_) => return Err(bad("file.save", "`trigger` is a short word such as close")),
    }
    // A PDF is a print of the sheet (Page Setup applies), not a copy of the workbook: the
    // document keeps its file and its unsaved changes.
    if std::path::Path::new(&path).extension().and_then(std::ffi::OsStr::to_str).is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
        return super::print::export_pdf(s, &json!({"path": path}));
    }
    // AutoSave (`ifUnchanged`) never overwrites a file another device or program changed since it
    // was read or written here: it reports the conflict for `file.resolveConflict` instead.
    let autosave_save = bool_param(p, "ifUnchanged") == Some(true);
    if autosave_save && let Some(conflict) = super::cloud::conflict_before_save(s, &path)? {
        return Ok(json!({"saved": false, "conflict": conflict.id(), "path": path}));
    }
    let d = s.doc_mut()?;
    let (old_path, autosave, uid) = (d.path.clone(), d.autosave, d.uid);
    // The document's own file keeps the format its content had when opened; any other path
    // goes by its extension. Relative paths to linked workbooks are rewritten for the new folder.
    let kind = crate::io::FileKind::from_path(&path).or(d.content_kind.filter(|_| d.path.as_deref() == Some(path.as_str())));
    let rebased = crate::io::source_dir(&path).and_then(|f| gridcraft_xlsx::rebase_links(&d.wb, &f));
    let wb = rebased.as_ref().unwrap_or(&d.wb);
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut bytes = match kind {
        Some(kind) => crate::io::save_bytes_as(wb, &path, kind)?,
        None => crate::io::save_bytes(wb, &path)?,
    };
    // Only a full-fidelity format clears the dirty flag and becomes the document's file.
    let own = matches!(kind, Some(crate::io::FileKind::Xlsx | crate::io::FileKind::Json));
    // The version history: settle the file's identity and keep what is on disk before it is
    // replaced. It can't fail the save.
    #[cfg(not(target_arch = "wasm32"))]
    let history = if own && s.has_versions() {
        let request = crate::versions::SaveRequest { autosave: autosave_save, trigger: str_param(p, "trigger"), conflict_copy };
        let pending = s.prepare_version(uid, std::path::Path::new(&path), request);
        if crate::versions::is_package(std::path::Path::new(&path)) {
            let id = pending.as_ref().and_then(|p| p.document_id.as_deref());
            bytes = match id {
                Some(id) => crate::versions::with_document_id(bytes, Some(id)),
                None => crate::versions::without_document_id(bytes),
            };
        }
        Some((request, pending))
    } else {
        None
    };
    #[cfg(target_arch = "wasm32")]
    let _ = (uid, conflict_copy);
    crate::io::write_file(&path, &bytes)?;
    let len = bytes.len();
    let d = s.doc_mut()?;
    if own {
        // The new file is the workbook's own now, so `CELL("filename")` reports it (#1489). Set
        // before the `saved` snapshot, or the workbook would read as dirty the moment it was
        // saved; and only when it changes, so an ordinary Save does not clone the workbook.
        let file = crate::io::source_path(&path);
        if d.wb.source_path != file {
            std::sync::Arc::make_mut(&mut d.wb).source_path = file;
        }
        d.saved = d.wb.clone();
        d.path = Some(path.clone());
        // The file as just written is what AutoSave guards from now on.
        d.stamp = gridcraft_cloud::DiskStamp::of_bytes(std::path::Path::new(&path), &bytes).ok();
        d.conflict = None;
        // Save As (and Save to Cloud, and keeping both) gives the new file the switch the workbook had.
        if old_path.as_deref() != Some(path.as_str()) {
            s.autosave_follows(&path, autosave);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some((request, pending)) = history {
            s.record_saved(uid, std::path::Path::new(&path), bytes, request, pending);
        }
    }
    Ok(json!({"saved": true, "path": path, "bytes": len}))
}

fn save_as(s: &mut Session, p: &Json) -> Result<Json> {
    if str_param(p, "path").is_none() {
        s.ui_requests.push(crate::UiRequest::Dialog("saveAs".into(), json!({})));
        return ok();
    }
    save(s, p)
}

fn save_bytes(s: &mut Session, p: &Json) -> Result<Json> {
    let fmt = str_param(p, "format").unwrap_or("xlsx");
    let d = s.doc()?;
    let name = format!("{}.{fmt}", std::path::Path::new(&d.display_title()).file_stem().and_then(|x| x.to_str()).unwrap_or("Book"));
    // The file name goes into a PivotChart's link to its PivotTable.
    let bytes = crate::io::save_bytes(&d.wb, &name)?;
    Ok(json!({"name": name, "base64": crate::io::base64_encode(&bytes), "bytes": bytes.len()}))
}

/// File › Close closes the active window; the workbook closes with its last window.
fn close(s: &mut Session, p: &Json) -> Result<Json> {
    // Closing a previous version stops there, as closing its window would: the current workbook
    // comes back as it was, and a further Close asks about its changes as usual.
    if let Some(d) = s.active().filter(|d| d.is_previous_version()) {
        let uid = d.uid;
        s.close_previous_version(uid);
        return Ok(json!({"closed": true, "version": true}));
    }
    if !bool_param(p, "document").unwrap_or(false)
        && let Some(id) = s.active_window()
        && let Some(doc) = s.window(id).map(|w| w.doc_uid)
        && s.windows().iter().filter(|w| w.doc_uid == doc).count() > 1
    {
        s.close_window(id);
        return Ok(json!({"closed": true, "document": false}));
    }
    let force = bool_param(p, "force").unwrap_or(false);
    let d = s.doc()?;
    if d.is_dirty() && !force {
        // With AutoSave on, the changes are written and the workbook closes without the question.
        match s.autosave_before_discard(d.uid) {
            super::cloud::Flush::Saved => {}
            super::cloud::Flush::Conflict(_) => return Ok(json!({"closed": false, "dirty": true, "conflict": true})),
            super::cloud::Flush::Ask => {
                let title = s.doc()?.display_title();
                s.ui_requests.push(crate::UiRequest::Dialog("saveChanges".into(), json!({"title": title})));
                return Ok(json!({"closed": false, "dirty": true}));
            }
        }
    }
    let i = s.active_index();
    s.close_document(i);
    Ok(json!({"closed": true}))
}

fn export(s: &mut Session, p: &Json, ext: &str) -> Result<Json> {
    let path = str_param(p, "path").ok_or_else(|| bad("file.export", "missing `path`"))?.to_string();
    let d = s.doc()?;
    let wb = (*d.wb).clone();
    if p.get("sheet").is_some() {
        wb.set_active_sheet(target_sheet(s, p)?);
    }
    let target = if path.ends_with(&format!(".{ext}")) { path.clone() } else { format!("{path}.{ext}") };
    let bytes = if ext == "csv" && bool_param(p, "values").unwrap_or(false) {
        crate::io::csv_values_bytes(&wb)?
    } else {
        crate::io::save_bytes(&wb, &target)?
    };
    crate::io::write_file(&target, &bytes)?;
    Ok(json!({"path": target, "bytes": bytes.len()}))
}

fn properties(s: &mut Session, p: &Json) -> Result<Json> {
    edit(s, |cx| {
        let pr = &mut cx.wb.props;
        for (k, slot) in [
            ("title", &mut pr.title),
            ("subject", &mut pr.subject),
            ("author", &mut pr.author),
            ("company", &mut pr.company),
            ("keywords", &mut pr.keywords),
            ("description", &mut pr.description),
        ] {
            if let Some(v) = str_param(p, k) {
                *slot = v.to_string();
            }
        }
        Ok(serde_json::to_value(&cx.wb.props).unwrap_or_default())
    })
}

/// File › Options: reports the workbook-level settings and asks the interface to show its Options
/// page. Nothing changes here; every setting has its own command.
fn options(s: &mut Session, p: &Json) -> Result<Json> {
    // File > Options > Save > Turn on AutoSave by default is application-wide, and the workbooks
    // follow it unless their file has a choice of its own. The version history folders and the
    // device name are application settings too.
    let history = HistoryOptions::read(p)?;
    let setting = history.any();
    if setting {
        history.apply(s)?;
    } else {
        s.ui_requests.push(crate::UiRequest::Dialog("options".into(), json!({})));
    }
    let calculation = s.active().map(|d| {
        let c = &d.wb.calc;
        let mode = match c.mode {
            gridcraft_model::CalcMode::Automatic => "automatic",
            gridcraft_model::CalcMode::AutomaticExceptTables => "automaticExceptTables",
            gridcraft_model::CalcMode::Manual => "manual",
        };
        json!({"mode": mode, "iterative": c.iterative, "maxIterations": c.max_iterations, "maxChange": c.max_change, "precisionAsDisplayed": c.precision_as_displayed})
    });
    let mut r = json!({"calculation": calculation});
    // Setting an application option reports the options it belongs to.
    if setting {
        r["autosaveByDefault"] = json!(s.prefs.autosave_by_default);
        HistoryOptions::report(s, &mut r);
    }
    Ok(r)
}

#[cfg(not(target_arch = "wasm32"))]
use crate::versions::{MAX_DEVICE_LABEL_CHARS, Relocation};

/// The browser keeps no history, so nothing is relocated.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
enum Relocation {
    Copy,
    Move,
    Leave,
}

/// Longest device label, in characters.
#[cfg(target_arch = "wasm32")]
const MAX_DEVICE_LABEL_CHARS: usize = 64;

/// The AutoSave default and the version history settings `file.options` reads from its parameters.
struct HistoryOptions {
    autosave_by_default: Option<bool>,
    /// `Some(None)`: back to the settings folder alone.
    versions_folder: Option<Option<String>>,
    versions_mirror: Option<bool>,
    existing: Relocation,
    /// `Some(None)`: the system's name.
    device_label: Option<Option<String>>,
}

impl HistoryOptions {
    fn read(p: &Json) -> Result<Self> {
        let versions_folder = match p.get("versionsFolder") {
            None | Some(Json::Null) => None,
            Some(Json::String(f)) if f.trim().is_empty() || f.trim().eq_ignore_ascii_case("default") => Some(None),
            Some(Json::String(f)) => Some(Some(folder_param(f)?)),
            Some(_) => return Err(bad("file.options", "`versionsFolder` must be a folder path, or default")),
        };
        let existing = match str_param(p, "versionsExisting") {
            None | Some("copy") => Relocation::Copy,
            Some("move") => Relocation::Move,
            Some("leave") => Relocation::Leave,
            Some(other) => return Err(bad("file.options", format!("`versionsExisting` is copy, move or leave, not `{other}`"))),
        };
        let device_label = match p.get("deviceLabel") {
            None | Some(Json::Null) => None,
            Some(Json::String(l)) if l.trim().is_empty() || l.trim().eq_ignore_ascii_case("auto") => Some(None),
            Some(Json::String(l)) => Some(Some(l.trim().chars().filter(|c| !c.is_control()).take(MAX_DEVICE_LABEL_CHARS).collect::<String>())),
            Some(_) => return Err(bad("file.options", "`deviceLabel` must be a string, or auto")),
        };
        Ok(Self {
            autosave_by_default: bool_param(p, "autosaveByDefault"),
            versions_folder,
            versions_mirror: bool_param(p, "versionsMirror"),
            existing,
            device_label,
        })
    }

    fn any(&self) -> bool {
        self.autosave_by_default.is_some() || self.versions_folder.is_some() || self.versions_mirror.is_some() || self.device_label.is_some()
    }

    fn apply(self, s: &mut Session) -> Result<()> {
        if let Some(on) = self.autosave_by_default {
            s.set_autosave_by_default(on);
        }
        if let Some(label) = self.device_label {
            s.prefs.device_label = label;
        }
        if self.versions_folder.is_some() || self.versions_mirror.is_some() {
            if let Some(folder) = self.versions_folder {
                s.prefs.versions_folder = folder;
            }
            if let Some(mirror) = self.versions_mirror {
                s.prefs.versions_mirror = mirror;
            }
            #[cfg(not(target_arch = "wasm32"))]
            s.apply_version_folders(self.existing).map_err(EngineError::Other)?;
            #[cfg(target_arch = "wasm32")]
            let _ = self.existing;
        }
        Ok(())
    }

    /// The history settings in `file.options`' result.
    fn report(s: &Session, r: &mut Json) {
        let pr = &s.prefs;
        #[cfg(not(target_arch = "wasm32"))]
        let (folders, unavailable, relocating) = s.version_folders();
        #[cfg(target_arch = "wasm32")]
        let (folders, unavailable, relocating): (Vec<std::path::PathBuf>, Vec<std::path::PathBuf>, bool) = (Vec::new(), Vec::new(), false);
        #[cfg(not(target_arch = "wasm32"))]
        let device_default = crate::versions::default_device_label();
        #[cfg(target_arch = "wasm32")]
        let device_default = "Browser";
        if let Some(o) = r.as_object_mut() {
            o.insert("versionsFolder".into(), json!(pr.versions_folder));
            o.insert("versionsMirror".into(), json!(pr.versions_mirror));
            o.insert("versionsFolders".into(), json!(folders));
            o.insert("versionsUnavailable".into(), json!(unavailable));
            o.insert("versionsRelocating".into(), json!(relocating));
            o.insert("deviceLabel".into(), json!(pr.device_label));
            o.insert("deviceLabelDefault".into(), json!(device_default));
        }
    }
}

/// A history folder setting: an existing folder, given as an absolute path or one starting with
/// `~/`.
fn folder_param(f: &str) -> Result<String> {
    let f = f.trim();
    if f.chars().count() > 1024 || f.chars().any(char::is_control) {
        return Err(bad("file.options", "`versionsFolder` isn't a folder path"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).filter(|h| !h.is_empty()).map(std::path::PathBuf::from);
    let path = match (f.strip_prefix("~/"), home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ => std::path::PathBuf::from(f),
    };
    if !path.is_absolute() {
        return Err(bad("file.options", format!("`versionsFolder`: {f:?} isn't a full path")));
    }
    if !path.is_dir() {
        return Err(EngineError::Other(format!("no folder at {}", path.display())));
    }
    Ok(path.to_string_lossy().into_owned())
}

fn activate_window(s: &mut Session, p: &Json) -> Result<Json> {
    let id = match u32_param(p, "index").filter(|_| p.get("window").is_none_or(Json::is_null) && p.get("doc").is_none_or(Json::is_null)) {
        Some(i) => {
            let uid = s.documents().get(i as usize).map(|d| d.uid).ok_or_else(|| bad("window.activate", "no such window"))?;
            s.doc_window(uid).ok_or_else(|| bad("window.activate", "no such window"))?
        }
        None if p.get("window").is_none_or(Json::is_null) && p.get("doc").is_none_or(Json::is_null) => {
            return Err(bad("window.activate", "missing `window`"));
        }
        None => super::extra::window_param(s, "window.activate", p)?,
    };
    s.unhide_window(id);
    ok()
}

fn close_window(s: &mut Session, p: &Json) -> Result<Json> {
    let id = super::extra::window_param(s, "window.close", p)?;
    let doc = s.window(id).map(|w| w.doc_uid).unwrap_or_default();
    if s.windows().iter().filter(|w| w.doc_uid == doc).count() > 1 {
        s.close_window(id);
        return Ok(json!({"closed": true, "document": false}));
    }
    let force = bool_param(p, "force").unwrap_or(false);
    let d = s.documents().iter().find(|d| d.uid == doc).ok_or(crate::EngineError::NoDocument)?;
    if d.is_dirty() && !force {
        let (title, uid) = (d.display_title(), d.uid);
        s.activate_window(id);
        // With AutoSave on, the changes are written and the workbook closes without the question.
        match s.autosave_before_discard(uid) {
            super::cloud::Flush::Saved => {}
            super::cloud::Flush::Conflict(_) => return Ok(json!({"closed": false, "dirty": true, "conflict": true})),
            super::cloud::Flush::Ask => {
                s.ui_requests.push(crate::UiRequest::Dialog("saveChanges".into(), json!({"title": title})));
                return Ok(json!({"closed": false, "dirty": true}));
            }
        }
    }
    s.close_window(id);
    Ok(json!({"closed": true, "document": true}))
}

/// Characters a file name may not have on any system we run on, plus path separators.
#[cfg(not(target_arch = "wasm32"))]
const BAD_NAME_CHARS: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*'];

#[cfg(not(target_arch = "wasm32"))]
/// The file name `typed` makes for a file that had extension `ext`: the extension is added when
/// the name does not already end with it. `None` for a name that cannot be a file name.
pub(crate) fn renamed_file_name(typed: &str, ext: Option<&str>) -> Option<String> {
    let name = typed.trim().trim_end_matches('.');
    if name.is_empty() || name == "." || name == ".." || name.chars().any(|c| c.is_control() || BAD_NAME_CHARS.contains(&c)) {
        return None;
    }
    let name = match ext {
        Some(ext) if !ext.is_empty() && !name.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) => format!("{name}.{ext}"),
        _ => name.to_string(),
    };
    (name.len() <= 255).then_some(name)
}

/// `file.rename`: the file moves to its new name; its history, AutoSave switch and disk stamp
/// follow. The browser has no files.
#[cfg(target_arch = "wasm32")]
fn rename(_: &mut Session, _: &Json) -> Result<Json> {
    Err(EngineError::Disabled("file.rename".into(), "files can only be renamed in the desktop app".into()))
}

#[cfg(not(target_arch = "wasm32"))]
fn rename(s: &mut Session, p: &Json) -> Result<Json> {
    use std::path::{Path, PathBuf};

    let typed = str_param(p, "name").ok_or_else(|| bad("file.rename", "missing `name`"))?;
    let d = s.doc()?;
    let uid = d.uid;
    let from = d.path.clone().ok_or_else(|| EngineError::Disabled("file.rename".into(), "save the workbook first: it has no file to rename".into()))?;
    let from_path = PathBuf::from(&from);
    let ext = from_path.extension().map(|e| e.to_string_lossy().into_owned());
    let name = renamed_file_name(typed, ext.as_deref())
        .ok_or_else(|| bad("file.rename", "that is not a file name: it needs a name without folders or the characters / \\ : * ? \" < > |"))?;
    let to_path = from_path.with_file_name(&name);
    let to = to_path.to_string_lossy().into_owned();
    if to == from {
        return Ok(json!({"renamed": false, "path": from, "from": from}));
    }
    if !crate::io::file_access_allowed(&to) {
        return Err(EngineError::Other(format!("{to}: scripts, and macro steps without the permission, can't read or write files")));
    }
    // A name only in its case is the same file on Windows and macOS: that rename is allowed.
    let same_file = crate::autosave::file_key(&from_path).is_some() && crate::autosave::file_key(&from_path) == crate::autosave::file_key(&to_path);
    if !same_file && (to_path.exists() || s.documents().iter().any(|o| o.path.as_deref().is_some_and(|op| Path::new(op) == to_path))) {
        return Err(EngineError::Other(format!("a file named {name} already exists in that folder")));
    }
    // The AutoSave choice is remembered under the file's canonical path: take it before the move.
    let canonical_from = std::fs::canonicalize(&from_path).unwrap_or_else(|_| from_path.clone());
    std::fs::rename(&from_path, &to_path).map_err(|e| EngineError::Other(format!("{from}: {e}")))?;
    s.autosave_memory.rename(&canonical_from, &to_path);
    let d = s.doc_mut()?;
    let dirty = d.is_dirty();
    d.path = Some(to.clone());
    if d.wb.source_path.is_some() {
        let file = crate::io::source_path(&to);
        std::sync::Arc::make_mut(&mut d.wb).source_path = file;
        if !dirty {
            d.saved = d.wb.clone();
        }
    }
    s.versions_renamed(uid, from_path, &to_path);
    Ok(json!({"renamed": true, "path": to, "from": from}))
}
