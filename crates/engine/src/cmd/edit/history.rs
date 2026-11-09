//! The commands behind the Clipboard task pane: list, paste one, paste all, delete, clear.

use super::paste_special::paste_special;
use super::{CellRef, EngineError, Json, Result, Session, bad, bool_param, cell_param_on, json, target_sheet};
use crate::Clipboard;

pub(super) fn history_items(s: &mut Session, _: &Json) -> Result<Json> {
    Ok(s.clip_history.describe())
}

fn index(s: &Session, p: &Json) -> Result<usize> {
    let n = p.get("index").and_then(Json::as_u64).ok_or_else(|| bad("edit.pasteClipboardItem", "`index` (number, 0 = newest) is required"))?;
    usize::try_from(n)
        .ok()
        .filter(|i| *i < s.clip_history.len())
        .ok_or_else(|| bad("edit.pasteClipboardItem", format!("no clipboard item {n} (there are {})", s.clip_history.len())))
}

/// Paste `clip` as a copy at `at` (the selection when `None`). On success it is the clipboard, so
/// a later Paste repeats it; on failure the clipboard stays as it was.
fn paste_clip(s: &mut Session, clip: &Clipboard, at: Option<CellRef>) -> Result<Json> {
    let kept = s.clipboard.replace(Clipboard { cut: false, ..clip.clone() });
    let at = at.map(|c| Json::String(c.a1())).unwrap_or(Json::Null);
    let r = paste_special(s, &json!({"what": "all", "at": at}));
    if r.is_err() {
        s.clipboard = kept;
    }
    r
}

/// The reply: what was pasted and the text (and HTML on request) the host puts on the system
/// clipboard, so the item is on it too.
fn reply(pasted: &Json, clip: &Clipboard, html: bool) -> Json {
    let mut out = json!({"pasted": pasted.get("pasted").cloned().unwrap_or(Json::Null), "text": clip.text});
    if html && let Some(h) = crate::io::range_html(&clip.wb, clip.sheet, clip.range, &clip.text) {
        out["html"] = Json::String(h);
    }
    out
}

pub(super) fn paste_item(s: &mut Session, p: &Json) -> Result<Json> {
    let i = index(s, p)?;
    let clip = s.clip_history.get(i).map(|it| it.clip.clone()).ok_or(EngineError::Other("No such clipboard item.".into()))?;
    let sheet = target_sheet(s, p)?;
    let at = cell_param_on(s, p, "at", sheet)?;
    let pasted = paste_clip(s, &clip, at)?;
    Ok(reply(&pasted, &clip, bool_param(p, "html") == Some(true)))
}

/// Every item, oldest first, one below the other from `at` (the selection's top-left cell), as one
/// undo step. The newest item ends up on the clipboard.
pub(super) fn paste_all(s: &mut Session, p: &Json) -> Result<Json> {
    let all: Vec<Clipboard> = s.clip_history.items().iter().rev().map(|it| it.clip.clone()).collect();
    let sheet = target_sheet(s, p)?;
    let first = cell_param_on(s, p, "at", sheet)?.unwrap_or(s.doc()?.selection.current().start);
    let pasted = s.grouped("Paste All", |s| {
        let mut at = first;
        let mut out = Json::Null;
        for clip in &all {
            out = paste_clip(s, clip, Some(at))?;
            let end = s.doc()?.selection.current().end;
            at = CellRef::new(end.row.saturating_add(1), first.col);
        }
        Ok(out)
    })?;
    let newest = all.last().cloned().ok_or(EngineError::Other("Nothing to paste.".into()))?;
    Ok(reply(&pasted, &newest, bool_param(p, "html") == Some(true)))
}

pub(super) fn delete_item(s: &mut Session, p: &Json) -> Result<Json> {
    let i = index(s, p)?;
    s.clip_history.remove(i);
    Ok(s.clip_history.describe())
}

pub(super) fn clear_history(s: &mut Session, _: &Json) -> Result<Json> {
    s.clip_history.clear();
    Ok(s.clip_history.describe())
}
