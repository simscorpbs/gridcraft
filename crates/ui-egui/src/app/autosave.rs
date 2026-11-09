//! AutoSave in the interface: the clock that writes changed workbooks, the title-bar switch, the
//! status-bar error while writes keep failing, and the Save Conflict and Save to Cloud questions
//! (docs/cloud.md). The decisions (what is covered, when to write, how long to wait after a
//! failure) are the engine's, in `gridcraft_engine::autosave`.

use gridcraft_engine::autosave::{AutoSaveBlock, Outcome};

use super::{Json, SheetApp, json};
use crate::i18n::Language;

/// Why AutoSave can't cover a workbook, in the interface language.
pub fn block_reason(lang: Language, block: &AutoSaveBlock) -> String {
    match block {
        AutoSaveBlock::Browser => lang.tr("AutoSave isn't available in the browser: it can't write to your files. Save downloads a copy.").into(),
        AutoSaveBlock::Unsaved => lang.tr("Save the workbook to turn on AutoSave").into(),
        AutoSaveBlock::Format { name } => {
            lang.tr("AutoSave only keeps .xlsx, .xlsm, .xltx and .xltm workbooks; {name} can't keep everything. Save as .xlsx to turn on AutoSave.").replace("{name}", name)
        }
        AutoSaveBlock::NotSavedHere => lang.tr("Save the workbook to turn on AutoSave (GridCraft couldn't check this file on disk)").into(),
        AutoSaveBlock::Conflict { missing: false } => {
            lang.tr("AutoSave is paused: this file was changed on another device. Click to choose which version to keep.").into()
        }
        AutoSaveBlock::PreviousVersion => lang.tr("AutoSave doesn't apply to a previous version.").into(),
        AutoSaveBlock::Conflict { missing: true } => {
            lang.tr("AutoSave is paused: this file was moved or deleted elsewhere. Click to choose what to do.").into()
        }
    }
}

impl SheetApp {
    /// Why AutoSave can't cover the active workbook, or `None` when it can.
    pub fn autosave_block(&self) -> Option<AutoSaveBlock> {
        self.session.active().and_then(gridcraft_engine::DocState::autosave_block)
    }

    /// Whether AutoSave writes the active workbook: its switch is on and nothing blocks it. This is
    /// what the title-bar switch shows.
    pub fn autosave_on(&self) -> bool {
        self.session.active().is_some_and(|d| self.session.autosave_covers(d.uid))
    }

    /// What the title-bar switch's tooltip says.
    pub fn autosave_tooltip(&self) -> String {
        let lang = self.ui.language;
        match self.autosave_block() {
            Some(block) => block_reason(lang, &block),
            None if self.autosave_on() => lang.tr("AutoSave: save after every change (workbooks saved to a file)").into(),
            None => lang.tr("AutoSave is off for this workbook. Click to turn it on.").into(),
        }
    }

    /// The title-bar switch was clicked: turn AutoSave on or off for the active workbook (and
    /// remember it for its file), or, where it can't be on, do what lifts the block: choose where
    /// a new workbook is kept, or decide about the waiting conflict.
    pub fn click_autosave_switch(&mut self) {
        let lang = self.ui.language;
        match self.autosave_block() {
            Some(AutoSaveBlock::Conflict { .. }) => self.show_save_conflict(),
            Some(AutoSaveBlock::Unsaved) => self.start_autosave_new(),
            Some(block) => self.toast = Some((block_reason(lang, &block), crate::now_ms())),
            None => {
                let on = !self.autosave_on();
                self.run_or_alert("file.autosave", json!({"value": on}));
            }
        }
    }

    /// File › Options › Save › Turn on AutoSave by default.
    pub fn set_autosave_by_default(&mut self, on: bool) {
        self.run_or_alert("file.options", json!({"autosaveByDefault": on}));
    }

    /// Turning AutoSave on for a new workbook: offer the cloud folders found (as Excel offers
    /// OneDrive), else Save As. Once saved, the switch is on for the file.
    pub fn start_autosave_new(&mut self) {
        let Some((uid, title)) = self.session.active().map(|d| (d.uid, d.display_title())) else { return };
        self.session.set_autosave(uid, true);
        if self.session.cloud_roots.is_empty() {
            self.open_dialog("saveAs", json!({}));
        } else {
            let name = std::path::Path::new(&title).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            self.open_dialog("saveToCloud", json!({"name": name}));
        }
    }

    /// Ask what to do about the conflict AutoSave found (the switch, AutoSave itself, closing).
    pub fn show_save_conflict(&mut self) {
        let Some(d) = self.session.active() else { return };
        let Some(conflict) = d.conflict else { return };
        let params = json!({"title": d.display_title(), "missing": conflict == gridcraft_engine::autosave::DiskState::Missing});
        self.open_dialog("saveConflict", params);
    }

    /// Look for the iCloud Drive, Google Drive and OneDrive folders on this computer, off the UI
    /// thread (on Windows it looks at every drive letter); `repaint` wakes the app when done. The
    /// desktop app calls this once; until it finishes, no cloud folder is offered.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn discover_cloud(&mut self, repaint: std::sync::Arc<dyn Fn() + Send + Sync>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("cloud-discovery".into()).spawn(move || {
            let roots = gridcraft_cloud::discover(&gridcraft_cloud::Env::current());
            if tx.send(roots).is_ok() {
                repaint();
            }
        });
        match spawned {
            Ok(_) => self.cloud_discovery = Some(rx),
            Err(e) => log::warn!("cloud folders not looked for: {e}"),
        }
    }

    /// The status-bar message while AutoSave's writes of the active workbook keep failing.
    pub fn autosave_failure_message(&self) -> Option<String> {
        let uid = self.session.active()?.uid;
        let failure = self.autosaver.failing(uid)?;
        let error = crate::clean_error(&failure.error);
        Some(self.ui.language.tr("AutoSave couldn't write the file: {error}. It will try again shortly.").replace("{error}", &error))
    }

    /// AutoSave: write the workbooks that have a file shortly after they change. This decision
    /// lives in `logic` so a hidden or logic-only frame still saves. A write that fails is shown
    /// in the status bar and tried again after a growing wait; the switch stays on.
    pub(super) fn autosave(&mut self, ctx: &egui::Context) {
        let now = crate::monotonic_ms();
        let idle = self.editor.is_none() && self.text_box_editor.is_none();
        if idle && self.autosaver.next_wake(&self.session, now) == Some(0.0) {
            self.store_view_state();
        }
        let outcomes = self.autosaver.tick(&mut self.session, now, idle, &mut |_, _| {});
        for outcome in outcomes {
            if let Outcome::Saved(uid) = outcome {
                self.autosave_saved(uid, ctx);
            }
        }
        self.ask_about_conflict();
        if let Some(wait) = self.autosaver.next_wake(&self.session, now) {
            // Keep checking even when no input arrives while the window is hidden.
            ctx.request_repaint_after(std::time::Duration::from_millis(wait.clamp(250.0, 2000.0) as u64));
        }
    }

    /// A write went through. If the Save Changes question was already open for that workbook, the
    /// changes are in the file: the question closes and the workbook closes, as if Save was chosen.
    fn autosave_saved(&mut self, uid: u64, ctx: &egui::Context) {
        let asking = self.dialog.as_ref().is_some_and(|d| d.name() == "saveChanges");
        if !asking || self.session.active().map(|d| d.uid) != Some(uid) {
            return;
        }
        let quitting = self.dialog.take().is_some_and(|d| d.values.get("quit").and_then(Json::as_bool) == Some(true));
        self.run_or_alert("file.close", json!({"force": true, "document": true}));
        if quitting {
            self.continue_quit(ctx);
        }
    }

    /// The active workbook has a conflict nobody was asked about yet: ask, once, when no other
    /// question is open (a Save Changes question gives way to it).
    fn ask_about_conflict(&mut self) {
        let waiting: Vec<u64> = self.session.documents().iter().filter(|d| d.conflict.is_some() && d.autosave).map(|d| d.uid).collect();
        self.conflict_asked.retain(|uid| waiting.contains(uid));
        let Some(uid) = self.session.active().map(|d| d.uid).filter(|uid| waiting.contains(uid) && !self.conflict_asked.contains(uid)) else {
            return;
        };
        if self.dialog.is_none() || self.dialog.as_ref().is_some_and(|d| d.name() == "saveChanges") {
            self.conflict_asked.insert(uid);
            self.dialog = None;
            self.show_save_conflict();
        }
    }
}
