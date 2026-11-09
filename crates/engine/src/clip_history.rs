//! The Clipboard task pane's list (Home › Clipboard dialog launcher): the copies and cuts made in
//! this session, newest first, kept in memory only.
//!
//! Each item is the full [`Clipboard`] payload of the copy (the workbook snapshot, the range, the
//! formulas and formats), so pasting an old item is as faithful as a normal paste. As in Excel, an
//! item that was cut is kept as a copy: the marching ants belong to the live clipboard, and an old
//! cut pastes without moving anything.

use serde_json::{Value as Json, json};

use crate::Clipboard;

/// Most items the pane keeps (Excel's Office Clipboard holds 24).
pub const CLIP_MAX_ITEMS: usize = 24;
/// A copy whose text is bigger than this is not collected (it still reaches the clipboard).
const CLIP_ITEM_BYTES: usize = 4 << 20;
/// All collected items' text together stay under this; the oldest go first.
const CLIP_TOTAL_BYTES: usize = 24 << 20;
/// Characters of preview per item.
const CLIP_PREVIEW_CHARS: usize = 120;
/// Rows of the copy the preview shows.
const CLIP_PREVIEW_ROWS: usize = 3;

/// One collected copy or cut.
#[derive(Clone, Debug)]
pub struct ClipItem {
    /// The clipboard as the copy left it, with `cut` cleared.
    pub clip: Clipboard,
    /// Whether it was a cut when collected (shown in the pane; pasting it is a copy).
    pub was_cut: bool,
}

impl ClipItem {
    /// The first cells' text on one line: rows are joined by " / ", cells by two spaces, and the
    /// result is cut at [`CLIP_PREVIEW_CHARS`] with an ellipsis.
    pub fn preview(&self) -> String {
        let rows: Vec<String> = self
            .clip
            .text
            .lines()
            .take(CLIP_PREVIEW_ROWS + 1)
            .map(|l| l.split('\t').map(str::trim).collect::<Vec<_>>().join("  ").trim().to_string())
            .collect();
        let more_rows = rows.len() > CLIP_PREVIEW_ROWS;
        let mut joined = rows.into_iter().take(CLIP_PREVIEW_ROWS).collect::<Vec<_>>().join(" / ");
        if more_rows {
            joined.push_str(" …");
        }
        if joined.chars().count() > CLIP_PREVIEW_CHARS {
            joined = joined.chars().take(CLIP_PREVIEW_CHARS).collect::<String>() + "…";
        }
        joined
    }

    /// The range as shown to people: `Sheet1!A1:B3`.
    pub fn place(&self) -> String {
        let sheet = self.clip.wb.sheet(self.clip.sheet).map(|s| s.name.clone()).unwrap_or_default();
        if sheet.is_empty() { self.clip.range.a1() } else { format!("{sheet}!{}", self.clip.range.a1()) }
    }
}

/// Items collected by Copy and Cut during the session, newest first. At most [`CLIP_MAX_ITEMS`];
/// oversized copies are skipped.
#[derive(Clone, Debug, Default)]
pub struct ClipHistory {
    items: Vec<ClipItem>,
}

impl ClipHistory {
    pub fn items(&self) -> &[ClipItem] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<&ClipItem> {
        self.items.get(index)
    }

    /// Collect a copy or cut (newest first). The same copy again (same unedited workbook, range
    /// and text) does not add a second item.
    pub fn push(&mut self, clip: &Clipboard) {
        if clip.text.len() > CLIP_ITEM_BYTES {
            return;
        }
        let same = |i: &ClipItem| {
            std::sync::Arc::ptr_eq(&i.clip.wb, &clip.wb) && i.clip.sheet == clip.sheet && i.clip.range == clip.range && i.clip.text == clip.text
        };
        if self.items.first().is_some_and(same) {
            if let Some(first) = self.items.first_mut() {
                first.was_cut = clip.cut;
            }
            return;
        }
        let mut stored = clip.clone();
        stored.cut = false;
        self.items.insert(0, ClipItem { clip: stored, was_cut: clip.cut });
        self.items.truncate(CLIP_MAX_ITEMS);
        while self.items.len() > 1 && self.items.iter().map(|i| i.clip.text.len()).sum::<usize>() > CLIP_TOTAL_BYTES {
            self.items.pop();
        }
    }

    pub fn remove(&mut self, index: usize) -> Option<ClipItem> {
        (index < self.items.len()).then(|| self.items.remove(index))
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// The items for agents and the pane, newest first.
    pub fn describe(&self) -> Json {
        Json::Array(
            self.items
                .iter()
                .enumerate()
                .map(|(i, it)| {
                    json!({
                        "index": i,
                        "preview": it.preview(),
                        "range": it.place(),
                        "rows": it.clip.range.height(),
                        "cols": it.clip.range.width(),
                        "cut": it.was_cut,
                    })
                })
                .collect(),
        )
    }
}
