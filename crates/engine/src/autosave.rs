//! AutoSave's switch for each file (see `docs/cloud.md`).
//!
//! AutoSave is on for every workbook whose file keeps everything, as the File › Options › Save
//! preference [`crate::Prefs::autosave_by_default`] says. Turning the switch the other way on a
//! file is remembered for that file, between runs, in a small JSON file in the settings folder
//! ([`crate::Session::enable_autosave_memory`]); turning it back to the default forgets it. The
//! choice follows the file through a rename, and Save As gives the new file the switch the document
//! had. Headless sessions remember only while they run; the browser has no files to remember.
//!
//! Whether AutoSave can write the file at all (a format that keeps everything, a disk stamp, no
//! conflict) is the front end's question; this module only keeps the user's choice.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use gridcraft_cloud::DiskState;

use crate::Session;

#[cfg(not(target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};

/// Most files remembered; the oldest choice is forgotten first.
pub const MAX_ENTRIES: usize = 2000;
/// Longest normalised path remembered, in bytes. A longer one keeps the default.
const MAX_KEY_BYTES: usize = 4096;
/// Largest memory file read back.
#[cfg(not(target_arch = "wasm32"))]
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// The memory file's format.
#[cfg(not(target_arch = "wasm32"))]
const VERSION: u32 = 1;

/// The choices the user made, oldest first, and where they are kept.
#[derive(Debug, Default)]
pub struct AutoSaveMemory {
    entries: Vec<(String, bool)>,
    /// `<settings_dir>/autosave.json` once the desktop host enabled it.
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<PathBuf>,
    /// The last error reading or writing the file, if any.
    pub error: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    files: Vec<StoredEntry>,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize, Deserialize)]
struct StoredEntry {
    path: String,
    autosave: bool,
}

/// The key a file is remembered by: its canonical path, without the `\\?\` prefix Windows'
/// canonical form carries, with `\` separators and ignoring case on Windows and macOS (whose
/// usual file systems ignore it). `None` for a path too long to keep.
pub fn file_key(path: &Path) -> Option<String> {
    let canonical: PathBuf = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let key = normalise(&canonical.to_string_lossy(), cfg!(windows), cfg!(any(windows, target_os = "macos")));
    (!key.is_empty() && key.len() <= MAX_KEY_BYTES).then_some(key)
}

/// [`file_key`]'s text rules without touching the disk: two spellings of one path (`C:/A.xlsx`,
/// `\\?\c:\a.xlsx`) give the same key on Windows. For lists such as Recent.
pub fn path_text_key(path: &str) -> String {
    normalise(path, cfg!(windows), cfg!(any(windows, target_os = "macos")))
}

/// [`file_key`]'s text rules, apart so tests can try Windows paths everywhere.
fn normalise(path: &str, windows: bool, fold_case: bool) -> String {
    let mut key = path.to_string();
    if windows {
        key = key.replace('/', "\\");
        if let Some(rest) = key.strip_prefix("\\\\?\\UNC\\") {
            key = format!("\\\\{rest}");
        } else if let Some(rest) = key.strip_prefix("\\\\?\\") {
            key = rest.to_string();
        }
    }
    if fold_case { key.to_lowercase() } else { key }
}

impl AutoSaveMemory {
    /// The switch the user left on `path`, when it differs from the default.
    pub fn get(&self, path: &Path) -> Option<bool> {
        let key = file_key(path)?;
        self.entries.iter().find(|(k, _)| *k == key).map(|(_, on)| *on)
    }

    /// Remember `on` for `path`; a choice that matches `default` is forgotten instead.
    pub fn remember(&mut self, path: &Path, on: bool, default: bool) {
        let Some(key) = file_key(path) else { return };
        let before = self.entries.len();
        self.entries.retain(|(k, _)| *k != key);
        if on == default && self.entries.len() == before {
            return;
        }
        if on != default {
            self.entries.push((key, on));
            self.trim();
        }
        self.persist();
    }

    /// The file at `old` is now at `new` (File › Info › Rename): its choice goes with it.
    pub fn rename(&mut self, old: &Path, new: &Path) {
        let (Some(old), Some(new)) = (file_key(old), file_key(new)) else { return };
        if old == new {
            return;
        }
        let Some(i) = self.entries.iter().position(|(k, _)| *k == old) else { return };
        let (_, on) = self.entries.remove(i);
        self.entries.retain(|(k, _)| *k != new);
        self.entries.push((new, on));
        self.persist();
    }

    /// How many files are remembered.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn trim(&mut self) {
        if let Some(extra) = self.entries.len().checked_sub(MAX_ENTRIES).filter(|n| *n > 0) {
            self.entries.drain(..extra);
        }
    }

    /// Read the choices kept in `file` (a missing or damaged file remembers nothing) and keep
    /// writing them there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(file: PathBuf) -> Self {
        let mut memory = AutoSaveMemory { entries: Vec::new(), file: None, error: None };
        match read_limited(&file) {
            Ok(Some(bytes)) => match serde_json::from_slice::<Stored>(&bytes) {
                Ok(stored) if stored.version == VERSION => {
                    for e in stored.files {
                        if !e.path.is_empty() && e.path.len() <= MAX_KEY_BYTES && !memory.entries.iter().any(|(k, _)| *k == e.path) {
                            memory.entries.push((e.path, e.autosave));
                        }
                    }
                    memory.trim();
                }
                Ok(_) => memory.error = Some(format!("{}: unknown version", file.display())),
                Err(e) => memory.error = Some(format!("{}: {e}", file.display())),
            },
            Ok(None) => {}
            Err(e) => memory.error = Some(e),
        }
        if let Some(e) = &memory.error {
            log::warn!("AutoSave choices not read: {e}");
        }
        memory.file = Some(file);
        memory
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn persist(&mut self) {
        let Some(file) = &self.file else { return };
        let stored =
            Stored { version: VERSION, files: self.entries.iter().map(|(path, on)| StoredEntry { path: path.clone(), autosave: *on }).collect() };
        let written = serde_json::to_vec_pretty(&stored).map_err(|e| e.to_string()).and_then(|bytes| write_atomic(file, &bytes));
        match written {
            Ok(()) => self.error = None,
            Err(e) => {
                log::warn!("AutoSave choices not saved to {}: {e}", file.display());
                self.error = Some(e);
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn persist(&mut self) {}
}

/// The file's bytes, `None` when there is no file, an error when it is too big or unreadable.
#[cfg(not(target_arch = "wasm32"))]
fn read_limited(file: &Path) -> Result<Option<Vec<u8>>, String> {
    use std::io::Read;
    let f = match std::fs::File::open(file) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", file.display())),
    };
    let mut bytes = Vec::new();
    f.take(MAX_FILE_BYTES.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", file.display()))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("{} is larger than {MAX_FILE_BYTES} bytes", file.display()));
    }
    Ok(Some(bytes))
}

/// Write `bytes` to a temporary file beside `path` and rename it over `path`, so an interrupted
/// write leaves the previous choices in place.
#[cfg(not(target_arch = "wasm32"))]
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "autosave.json".into());
    let tmp = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.map_err(|e| format!("{}: {e}", path.display()))
}

/// Whether saving to `path` keeps the whole workbook: the formats [`crate::cmd`]'s `file.save`
/// treats as the workbook's own file (`.xlsx`, `.xlsm`, `.xltx`, `.xltm` and GridCraft's own JSON).
/// A csv, text or web page loses styles, formulas or sheets, so AutoSave leaves it alone.
/// `content_kind` is the format a file with an unknown extension was recognised as.
pub fn keeps_everything(path: &str, content_kind: Option<crate::io::FileKind>) -> bool {
    matches!(crate::io::FileKind::from_path(path).or(content_kind), Some(crate::io::FileKind::Xlsx | crate::io::FileKind::Json))
}

/// Why AutoSave can't cover a workbook ([`crate::DocState::autosave_block`]); the front end greys
/// the switch out and puts the reason in its tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AutoSaveBlock {
    /// The web app: saving is a download, there is no file to keep writing to.
    Browser,
    /// A new workbook that has never been saved.
    Unsaved,
    /// A file in a format that can't keep everything (csv, text, web page…), by file name.
    Format { name: String },
    /// A file AutoSave can't check before writing: opened from bytes, or imported; saving it lifts
    /// the block.
    NotSavedHere,
    /// Another device or program changed the file (or moved or deleted it, `missing`) since it was
    /// last read or written here; AutoSave waits for the user to choose what to keep.
    Conflict { missing: bool },
    /// A previous version opened from Version History: it has no file of its own and cannot change.
    PreviousVersion,
}

impl AutoSaveBlock {
    /// What a script is told, in English (the interface translates its own).
    pub fn reason(&self) -> String {
        match self {
            AutoSaveBlock::Browser => "AutoSave isn't available in the browser: it can't write to your files. Save downloads a copy.".into(),
            AutoSaveBlock::Unsaved => "Save the workbook to turn on AutoSave".into(),
            AutoSaveBlock::Format { name } => {
                format!("AutoSave only keeps .xlsx, .xlsm, .xltx and .xltm workbooks; {name} can't keep everything. Save as .xlsx to turn on AutoSave.")
            }
            AutoSaveBlock::NotSavedHere => "Save the workbook to turn on AutoSave (GridCraft couldn't check this file on disk)".into(),
            AutoSaveBlock::Conflict { missing: false } => {
                "AutoSave is paused: this file was changed on another device. Choose which version to keep.".into()
            }
            AutoSaveBlock::Conflict { missing: true } => "AutoSave is paused: this file was moved or deleted elsewhere. Choose what to do.".into(),
            AutoSaveBlock::PreviousVersion => "AutoSave doesn't apply to a previous version.".into(),
        }
    }

    /// Whether Save As can lift the block (everywhere but the browser, and not while a conflict waits).
    pub fn save_as_helps(&self) -> bool {
        !matches!(self, AutoSaveBlock::Browser | AutoSaveBlock::Conflict { .. } | AutoSaveBlock::PreviousVersion)
    }
}

impl crate::DocState {
    /// Why AutoSave can't write this workbook's file, or `None` when it can: the file was read
    /// from disk or saved here (so there is a disk stamp to check), in a format that keeps
    /// everything, with no conflict waiting. The browser cannot write files.
    pub fn autosave_block(&self) -> Option<AutoSaveBlock> {
        if cfg!(target_arch = "wasm32") {
            return Some(AutoSaveBlock::Browser);
        }
        if self.version.is_some() {
            return Some(AutoSaveBlock::PreviousVersion);
        }
        let Some(path) = self.path.as_deref() else { return Some(AutoSaveBlock::Unsaved) };
        if !keeps_everything(path, self.content_kind) {
            let name = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Some(AutoSaveBlock::Format { name });
        }
        if self.stamp.is_none() && self.conflict.is_none() {
            return Some(AutoSaveBlock::NotSavedHere);
        }
        self.conflict.map(|c| AutoSaveBlock::Conflict { missing: c == DiskState::Missing })
    }
}

/// How a write of one workbook's file by AutoSave ended ([`Session::autosave_write`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoSaveWrite {
    /// The file was written and the workbook is clean.
    Saved,
    /// Nothing was written: the file changed or went missing elsewhere (the conflict is recorded).
    Conflict(DiskState),
    /// AutoSave does not cover the workbook (switch off, no file, a format that loses things).
    NotCovered,
}

impl Session {
    /// Desktop host wiring: remember AutoSave choices in `file` (`<settings_dir>/autosave.json`)
    /// from now on, and give the open workbooks the switch remembered for their files.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_autosave_memory(&mut self, file: PathBuf) {
        self.autosave_memory = AutoSaveMemory::load(file);
        self.refresh_autosave();
    }

    /// The switch for a workbook whose file is `path`: what is remembered for it, else the default.
    pub fn autosave_for(&self, path: Option<&str>) -> bool {
        path.and_then(|p| self.autosave_memory.get(Path::new(p))).unwrap_or(self.prefs.autosave_by_default)
    }

    /// The switch remembered for the file of the workbook `uid`, when it differs from the default.
    pub fn autosave_remembered(&self, uid: u64) -> Option<bool> {
        let path = self.documents().iter().find(|d| d.uid == uid)?.path.as_deref()?;
        self.autosave_memory.get(Path::new(path))
    }

    /// Set every workbook's switch from what is remembered for its file, else the default.
    pub fn refresh_autosave(&mut self) {
        let switches: Vec<(u64, bool)> = self.documents().iter().map(|d| (d.uid, self.autosave_for(d.path.as_deref()))).collect();
        for (uid, on) in switches {
            if let Some(d) = self.doc_by_uid_mut(uid) {
                d.autosave = on;
            }
        }
    }

    /// The AutoSave switch of workbook `uid` (`file.autosave`), remembered for its file.
    pub fn set_autosave(&mut self, uid: u64, on: bool) {
        let default = self.prefs.autosave_by_default;
        let Some(d) = self.doc_by_uid_mut(uid) else { return };
        d.autosave = on;
        if let Some(path) = d.path.clone() {
            self.autosave_memory.remember(Path::new(&path), on, default);
        }
    }

    /// File > Options > Save: whether AutoSave is on for files with no choice of their own. The
    /// open workbooks follow it unless their file has one.
    pub fn set_autosave_by_default(&mut self, on: bool) {
        self.prefs.autosave_by_default = on;
        self.refresh_autosave();
    }

    /// A workbook just took `path` as its file (Save As, Save to Cloud, Keep both): the new file
    /// gets the switch the workbook had.
    pub(crate) fn autosave_follows(&mut self, path: &str, on: bool) {
        let default = self.prefs.autosave_by_default;
        self.autosave_memory.remember(Path::new(path), on, default);
    }

    /// Whether AutoSave writes workbook `uid`: its switch is on and nothing blocks it.
    pub fn autosave_covers(&self, uid: u64) -> bool {
        self.documents().iter().find(|d| d.uid == uid).is_some_and(|d| d.autosave && d.autosave_block().is_none())
    }

    /// Write workbook `uid`'s own file the way AutoSave does: `file.save` with `ifUnchanged`, so a
    /// version another device or program saved meanwhile is never overwritten. The active
    /// workbook stays active. A workbook AutoSave does not cover is left alone.
    pub fn autosave_write(&mut self, uid: u64) -> std::result::Result<AutoSaveWrite, String> {
        self.autosave_write_for(uid, None)
    }

    /// [`Session::autosave_write`] for a reason: `close` is the last write before the workbook
    /// closes, which the version history records as the trigger of that version.
    pub fn autosave_write_for(&mut self, uid: u64, trigger: Option<&str>) -> std::result::Result<AutoSaveWrite, String> {
        let Some(index) = self.document_index(uid) else { return Ok(AutoSaveWrite::NotCovered) };
        if !self.autosave_covers(uid) {
            return Ok(AutoSaveWrite::NotCovered);
        }
        let previous = self.active_uid();
        self.active = index;
        let mut params = serde_json::json!({"ifUnchanged": true});
        if let Some(trigger) = trigger {
            params["trigger"] = serde_json::json!(trigger);
        }
        let result = self.run("file.save", params);
        if let Some(back) = previous.and_then(|p| self.document_index(p)) {
            self.active = back;
        }
        let saved = result.map_err(|e| e.to_string())?;
        Ok(match saved.get("conflict").and_then(serde_json::Value::as_str) {
            Some("missing") => AutoSaveWrite::Conflict(DiskState::Missing),
            Some(_) => AutoSaveWrite::Conflict(DiskState::Changed),
            None => AutoSaveWrite::Saved,
        })
    }

    fn active_uid(&self) -> Option<u64> {
        self.active().map(|d| d.uid)
    }

    fn doc_by_uid_mut(&mut self, uid: u64) -> Option<&mut crate::DocState> {
        let i = self.document_index(uid)?;
        self.docs.get_mut(i)
    }
}

/// How long after one write AutoSave waits before the next, in milliseconds.
pub const DEBOUNCE_MS: f64 = 2000.0;

/// AutoSave writes of one workbook failing one after another (a file another program holds, a
/// full disk): the switch stays on and the write is tried again after a growing wait.
#[derive(Clone, Debug, PartialEq)]
pub struct Failure {
    /// How many writes in a row failed.
    pub failures: u32,
    /// When (in the scheduler's clock, milliseconds) the next write may be tried.
    pub retry_at: f64,
    /// The last error, as the status bar shows it.
    pub error: String,
}

impl Failure {
    /// The wait before trying again after `failures` failed writes: 5 s, doubling to at most a minute.
    pub fn retry_after(failures: u32) -> f64 {
        let doublings = failures.saturating_sub(1).min(4);
        (5_000.0 * f64::from(1u32 << doublings)).min(60_000.0)
    }
}

/// What a scheduler tick did for one workbook.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// The workbook's file was written.
    Saved(u64),
    /// Nothing was written: the file changed or went missing elsewhere. The front end asks what to keep.
    Conflict(u64, DiskState),
    /// The write failed (the error, and how many in a row); it is tried again after a wait.
    Failed { uid: u64, error: String, failures: u32 },
}

/// AutoSave's clock: decides, workbook by workbook, when a changed file is written. The caller
/// passes the time in milliseconds (any monotonic clock), so tests drive it without waiting.
#[derive(Debug, Default)]
pub struct Scheduler {
    last_write: HashMap<u64, f64>,
    failing: HashMap<u64, Failure>,
}

impl Scheduler {
    /// The failure AutoSave is in the middle of for workbook `uid`, while its writes keep failing.
    pub fn failing(&self, uid: u64) -> Option<&Failure> {
        self.failing.get(&uid)
    }

    /// Forget what is known about workbook `uid` (it closed, or its file changed hands).
    pub fn forget(&mut self, uid: u64) {
        self.last_write.remove(&uid);
        self.failing.remove(&uid);
    }

    /// Write every changed workbook AutoSave covers, at most once per [`DEBOUNCE_MS`] and not while
    /// a failed write waits to be retried. `idle` is false while the person is in the middle of
    /// an edit (a cell or text box being typed into), when nothing is written. `before` runs just
    /// before a workbook is written (the front end stores its view state there).
    pub fn tick(&mut self, s: &mut Session, now: f64, idle: bool, before: &mut dyn FnMut(&mut Session, u64)) -> Vec<Outcome> {
        // Workbooks closed or saved by hand since the last failure are no longer failing.
        self.failing.retain(|uid, _| s.documents().iter().any(|d| d.uid == *uid && d.is_dirty()));
        self.last_write.retain(|uid, _| s.document_index(*uid).is_some());
        let mut outcomes = vec![];
        if !idle {
            return outcomes;
        }
        let due: Vec<u64> = s
            .documents()
            .iter()
            .filter(|d| d.is_dirty() && s.autosave_covers(d.uid))
            .map(|d| d.uid)
            .filter(|uid| self.failing.get(uid).is_none_or(|f| now >= f.retry_at))
            .filter(|uid| self.last_write.get(uid).is_none_or(|last| now - last > DEBOUNCE_MS))
            .collect();
        for uid in due {
            self.last_write.insert(uid, now);
            before(s, uid);
            match s.autosave_write(uid) {
                Ok(AutoSaveWrite::Saved) => {
                    self.failing.remove(&uid);
                    outcomes.push(Outcome::Saved(uid));
                }
                Ok(AutoSaveWrite::Conflict(state)) => {
                    self.failing.remove(&uid);
                    outcomes.push(Outcome::Conflict(uid, state));
                }
                Ok(AutoSaveWrite::NotCovered) => {}
                Err(error) => {
                    let failures = self.failing.get(&uid).map_or(0, |f| f.failures).saturating_add(1);
                    log::warn!("AutoSave failed ({failures} in a row): {error}");
                    self.failing.insert(uid, Failure { failures, retry_at: now + Failure::retry_after(failures), error: error.clone() });
                    outcomes.push(Outcome::Failed { uid, error, failures });
                }
            }
        }
        outcomes
    }

    /// When the next tick has something to do, in milliseconds from `now`: `None` when no workbook
    /// waits to be written. The front end asks for a repaint then, so a hidden window still saves.
    pub fn next_wake(&self, s: &Session, now: f64) -> Option<f64> {
        s.documents()
            .iter()
            .filter(|d| d.is_dirty() && s.autosave_covers(d.uid))
            .map(|d| {
                let debounce = self.last_write.get(&d.uid).map_or(0.0, |last| (last + DEBOUNCE_MS - now).max(0.0));
                let retry = self.failing.get(&d.uid).map_or(0.0, |f| (f.retry_at - now).max(0.0));
                debounce.max(retry)
            })
            .reduce(f64::min)
    }
}
