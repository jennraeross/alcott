use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use ratatui::layout::Rect;

use crate::buffer::SheetBuffer;
use crate::config;
use crate::library::Library;
use crate::stats::Daily;
use crate::words;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Library,
    Sheets,
    Editor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    File,
    Edit,
    View,
    Export,
}

impl MenuKind {
    pub fn title(&self) -> &'static str {
        match self {
            MenuKind::File => "File",
            MenuKind::Edit => "Edit",
            MenuKind::View => "View",
            MenuKind::Export => "Xport",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportPromptKind {
    OutputPath,
    ImportCss,
    AttachFont,
    NewStyle,
}

#[derive(Debug, Clone)]
pub struct ExportPrompt {
    pub kind: ExportPromptKind,
    pub input: String,
}

/// Inline prompt for creating a new sheet (File menu: N).
#[derive(Debug, Clone, Default)]
pub struct NewSheetPrompt {
    pub input: String,
}

/// Inline prompt for in-buffer find (Edit menu: F, or Ctrl+G anywhere).
/// Enter jumps to the next match and leaves the prompt open for repeated
/// jumps; Esc closes it.
#[derive(Debug, Clone, Default)]
pub struct SearchPrompt {
    pub input: String,
}

#[derive(Debug, Clone)]
pub enum GroupPromptKind {
    /// Create a subgroup under `parent` (empty = library root).
    New { parent: PathBuf },
    /// Rename `original` in place.
    Rename { original: PathBuf },
}

#[derive(Debug, Clone)]
pub struct GroupPrompt {
    pub kind: GroupPromptKind,
    pub input: String,
}

use crate::buffer::{atomic_write, insert_char_at, insert_newline_at, History};
use crate::library::{group_key, slugify};
use crate::words::count_words_rope;

/// Items lifted for a move (`M`): navigate, `Enter` drops into the group.
#[derive(Debug, Clone, Default)]
pub struct MoveLift {
    /// group rel paths
    pub groups: Vec<PathBuf>,
    /// sheet abs paths
    pub sheets: Vec<PathBuf>,
}

impl MoveLift {
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty() && self.sheets.is_empty()
    }

    pub fn count(&self) -> usize {
        self.groups.len() + self.sheets.len()
    }
}

/// One displayed editor row, for click mapping (`editor_map` / `mv_map`).
/// `line` is the rope line (global for single buffers), `char_start` the
/// offset within that line where the visual row begins, `text` its plain
/// content (styles excluded, so columns can be recovered exactly).
#[derive(Debug, Clone, Default)]
pub struct EditorMapRow {
    pub line: usize,
    pub char_start: usize,
    pub text: String,
}

/// Multi-view click row: like [`EditorMapRow`] but addressed by segment.
/// Divider rows resolve to the following segment's start (matching `locate`).
#[derive(Debug, Clone, Default)]
pub struct MvMapRow {
    pub seg: usize,
    pub line: usize,
    pub char_start: usize,
    pub text: String,
}

/// One file inside a [`MultiView`].
#[derive(Debug)]
pub struct Segment {
    pub path: PathBuf,
    pub rope: ropey::Rope,
    pub dirty: bool,
    pub history: History,
}

impl Segment {
    pub fn words(&self) -> usize {
        count_words_rope(&self.rope)
    }

    /// (line, col) for a char index.
    pub fn line_col(&self, c: usize) -> (usize, usize) {
        let c = c.min(self.rope.len_chars());
        let line = self.rope.char_to_line(c);
        (line, c - self.rope.line_to_char(line))
    }

    /// Char index, clamped to the line (excluding its newline).
    pub fn clamp_to_line(&self, line: usize, col: usize) -> usize {
        let lines = self.rope.len_lines().max(1);
        let line = line.min(lines - 1);
        let start = self.rope.line_to_char(line);
        let mut len = self.rope.line(line).len_chars();
        if len > 0 && self.rope.get_char(start + len - 1) == Some('\n') {
            len -= 1;
        }
        start + col.min(len)
    }
}

/// Sequential multi-sheet editor. Dividers between segments are purely visual
/// (never part of any rope), so saving can never corrupt file boundaries:
/// each dirty segment writes atomically to its own file.
#[derive(Debug, Default)]
pub struct MultiView {
    pub segments: Vec<Segment>,
    /// (segment index, char index within that segment)
    pub cursor: (usize, usize),
    /// global line offset (buffer lines + divider rows)
    pub scroll: u16,
    /// First flattened line in the render window (see `flat index` in ui.rs):
    /// segment lines and divider rows concatenated; the visual scroll offset
    /// (`scroll`) is measured from here.
    pub anchor: usize,
}

/// Cap on combined multi-view size (bytes read from disk).
pub const MULTIVIEW_BYTES_CAP: u64 = 1_000_000;

impl MultiView {
    /// Global line where segment `seg` starts (incl. divider rows before it).
    pub fn seg_start_line(&self, seg: usize) -> usize {
        let mut line = 0;
        for s in self.segments.iter().take(seg) {
            line += s.rope.len_lines() + 1; // +1 divider row
        }
        line
    }

    /// Total flattened rows: segment lines plus one divider row before every
    /// segment after the first.
    pub fn flat_len(&self) -> usize {
        if self.segments.is_empty() {
            return 0;
        }
        let last = self.segments.len() - 1;
        self.seg_start_line(last) + self.segments[last].rope.len_lines().max(1)
    }

    /// Resolve a flat index to `(segment, line)`; `None` line is a divider
    /// row. Returns `None` past the end.
    pub fn flat_at(&self, flat: usize) -> Option<(usize, Option<usize>)> {
        let mut f = flat;
        for (i, s) in self.segments.iter().enumerate() {
            if i > 0 {
                if f == 0 {
                    return Some((i, None));
                }
                f -= 1;
            }
            let n = s.rope.len_lines().max(1);
            if f < n {
                return Some((i, Some(f.min(s.rope.len_lines().saturating_sub(1)))));
            }
            f -= n;
        }
        None
    }

    /// Flat index of segment `seg`'s `line` (same space as `seg_start_line`).
    pub fn flat_of(&self, seg: usize, line: usize) -> usize {
        self.seg_start_line(seg.min(self.segments.len().saturating_sub(1))) + line
    }

    pub fn words(&self) -> usize {
        self.segments.iter().map(|s| s.words()).sum()
    }

    pub fn is_dirty(&self) -> bool {
        self.segments.iter().any(|s| s.dirty)
    }

    /// Map a global content line to (segment, line-in-segment).
    /// Divider rows map to the start of the following segment.
    pub fn locate(&self, gline: usize) -> Option<(usize, usize)> {
        for (i, s) in self.segments.iter().enumerate() {
            let start = self.seg_start_line(i);
            let n = s.rope.len_lines();
            if gline < start + n {
                return Some((i, gline - start));
            }
            if gline == start + n {
                // divider row → start of next segment
                let next = (i + 1).min(self.segments.len() - 1);
                return Some((next, 0));
            }
        }
        None
    }

    pub fn active_mut(&mut self) -> Option<(&mut Segment, usize)> {
        let (seg, c) = self.cursor;
        let s = self.segments.get_mut(seg)?;
        let c = c.min(s.rope.len_chars());
        Some((s, c))
    }

    pub fn insert_char(&mut self, c: char) -> bool {
        let Some((s, mut cur)) = self.active_mut() else {
            return false;
        };
        let pos = cur.min(s.rope.len_chars());
        let done = insert_char_at(&mut s.rope, &mut cur, c);
        s.history.record_insert(pos, c.to_string(), true);
        s.dirty = true;
        self.cursor.1 = cur;
        done
    }

    pub fn insert_newline(&mut self) -> bool {
        let (seg, c) = self.cursor;
        let line = {
            let Some(s) = self.segments.get(seg) else {
                return false;
            };
            let c = c.min(s.rope.len_chars());
            s.rope.line(s.rope.char_to_line(c)).to_string()
        };
        let Some((s, mut cur)) = self.active_mut() else {
            return false;
        };
        let pos = cur.min(s.rope.len_chars());
        let done = insert_newline_at(&mut s.rope, &mut cur, &line);
        let text: String = s.rope.slice(pos..cur).chars().collect();
        s.history.record_insert(pos, text, false);
        s.dirty = true;
        self.cursor.1 = cur;
        done
    }

    pub fn backspace(&mut self) {
        let (seg, c) = self.cursor;
        let Some(s) = self.segments.get_mut(seg) else {
            return;
        };
        let c = c.min(s.rope.len_chars());
        if c == 0 {
            // Never merge files: hop to the end of the previous segment.
            if seg > 0 {
                let prev = seg - 1;
                let end = self.segments[prev].rope.len_chars();
                // drop trailing newline from the landing spot
                let end = if end > 0
                    && self.segments[prev].rope.get_char(end - 1) == Some('\n')
                {
                    end - 1
                } else {
                    end
                };
                self.cursor = (prev, end);
            }
            return;
        }
        if let Some(ch) = s.rope.get_char(c - 1) {
            s.history.record_delete(c - 1, ch.to_string(), true);
        }
        s.rope.remove(c - 1..c);
        s.dirty = true;
        self.cursor.1 = c - 1;
    }

    pub fn delete_forward(&mut self) {
        let (seg, c) = self.cursor;
        let Some(s) = self.segments.get_mut(seg) else {
            return;
        };
        let c = c.min(s.rope.len_chars());
        if c >= s.rope.len_chars() {
            // Never merge files: hop to the start of the next segment.
            if seg + 1 < self.segments.len() {
                self.cursor = (seg + 1, 0);
            }
            return;
        }
        if let Some(ch) = s.rope.get_char(c) {
            s.history.record_delete(c, ch.to_string(), false);
        }
        s.rope.remove(c..c + 1);
        s.dirty = true;
    }

    pub fn move_left(&mut self) {
        let (seg, c) = self.cursor;
        if c > 0 {
            self.cursor.1 = c - 1;
        } else if seg > 0 {
            let prev = seg - 1;
            self.cursor = (prev, self.segments[prev].rope.len_chars());
        }
    }

    pub fn move_right(&mut self) {
        let (seg, c) = self.cursor;
        let len = self.segments.get(seg).map(|s| s.rope.len_chars()).unwrap_or(0);
        if c < len {
            self.cursor.1 = c + 1;
        } else if seg + 1 < self.segments.len() {
            self.cursor = (seg + 1, 0);
        }
    }

    pub fn move_up(&mut self) {
        let (seg, c) = self.cursor;
        let Some(s) = self.segments.get(seg) else {
            return;
        };
        let c = c.min(s.rope.len_chars());
        let line = s.rope.char_to_line(c);
        let col = c - s.rope.line_to_char(line);
        if line > 0 {
            self.cursor.1 = s.clamp_to_line(line - 1, col);
        } else if seg > 0 {
            let prev = seg - 1;
            let last = self.segments[prev].rope.len_lines().saturating_sub(1);
            self.cursor = (prev, self.segments[prev].clamp_to_line(last, col));
        }
    }

    /// Up one visual (wrapped) row; hops segment boundaries like [`move_up`].
    /// Falls back to a rope-line hop when the width is unknown (0).
    pub fn move_up_visual(&mut self, width: u16) {
        if width == 0 {
            self.move_up();
            return;
        }
        let (seg, c) = self.cursor;
        let Some(s) = self.segments.get(seg) else {
            return;
        };
        let c = c.min(s.rope.len_chars());
        let line = s.rope.char_to_line(c);
        let col = c - s.rope.line_to_char(line);
        let text = crate::view::strip_newline(&s.rope.line(line).to_string()).to_string();
        let rows = crate::view::wrap_geometry(&text, width);
        let mut row = rows.len().saturating_sub(1);
        for (i, (start, _)) in rows.iter().enumerate() {
            if *start > col {
                break;
            }
            row = i;
        }
        let (rs, rl) = rows.get(row).copied().unwrap_or((0, 0));
        let col = col.min(rs + rl);
        if row > 0 {
            // Same-line hop: keep the visual column relative to the row
            // start, clamped into the target row (mirrors visual_down).
            let (ps, pl) = rows[row - 1];
            let rel = col.saturating_sub(rs);
            self.cursor.1 = self.segments[seg].clamp_to_line(line, ps + rel.min(pl));
            return;
        }
        if line > 0 {
            let prev = crate::view::strip_newline(&self.segments[seg].rope.line(line - 1).to_string()).to_string();
            let prows = crate::view::wrap_geometry(&prev, width);
            let (ps, pl) = prows.last().copied().unwrap_or((0, 0));
            self.cursor.1 = self.segments[seg].clamp_to_line(line - 1, col.min(ps + pl));
        } else if seg > 0 {
            let prev = seg - 1;
            if self.segments[prev].rope.len_lines() == 0 {
                self.cursor = (prev, 0);
                return;
            }
            let last = self.segments[prev].rope.len_lines() - 1;
            let ptext = crate::view::strip_newline(&self.segments[prev].rope.line(last).to_string()).to_string();
            let prows = crate::view::wrap_geometry(&ptext, width);
            let (ps, pl) = prows.last().copied().unwrap_or((0, 0));
            self.cursor = (prev, self.segments[prev].clamp_to_line(last, col.min(ps + pl)));
        }
    }

    pub fn move_down(&mut self) {
        let (seg, c) = self.cursor;
        let Some(s) = self.segments.get(seg) else {
            return;
        };
        let c = c.min(s.rope.len_chars());
        let line = s.rope.char_to_line(c);
        let col = c - s.rope.line_to_char(line);
        if line + 1 < s.rope.len_lines() {
            self.cursor.1 = s.clamp_to_line(line + 1, col);
        } else if seg + 1 < self.segments.len() {
            self.cursor = (seg + 1, self.segments[seg + 1].clamp_to_line(0, col));
        }
    }

    /// Down one visual (wrapped) row; hops segment boundaries like
    /// [`move_down`]. Falls back to a rope-line hop when width is 0.
    pub fn move_down_visual(&mut self, width: u16) {
        if width == 0 {
            self.move_down();
            return;
        }
        let (seg, c) = self.cursor;
        let Some(s) = self.segments.get(seg) else {
            return;
        };
        let c = c.min(s.rope.len_chars());
        let line = s.rope.char_to_line(c);
        let col = c - s.rope.line_to_char(line);
        let text = crate::view::strip_newline(&s.rope.line(line).to_string()).to_string();
        let rows = crate::view::wrap_geometry(&text, width);
        let mut row = rows.len().saturating_sub(1);
        for (i, (start, _)) in rows.iter().enumerate() {
            if *start > col {
                break;
            }
            row = i;
        }
        let (rs, rl) = rows.get(row).copied().unwrap_or((0, 0));
        let col = col.min(rs + rl);
        if row + 1 < rows.len() {
            let (ns, nl) = rows[row + 1];
            let rel = col.saturating_sub(rs);
            self.cursor.1 = self.segments[seg].clamp_to_line(line, ns + rel.min(nl));
            return;
        }
        if line + 1 < self.segments[seg].rope.len_lines() {
            let ntext = crate::view::strip_newline(&self.segments[seg].rope.line(line + 1).to_string()).to_string();
            let nrows = crate::view::wrap_geometry(&ntext, width);
            let (ns, nl) = nrows.first().copied().unwrap_or((0, 0));
            self.cursor.1 = self.segments[seg].clamp_to_line(line + 1, col.min(ns + nl));
        } else if seg + 1 < self.segments.len() {
            self.cursor = (seg + 1, self.segments[seg + 1].clamp_to_line(0, col));
        }
    }

    pub fn move_home(&mut self) {
        let (seg, c) = self.cursor;
        if let Some(s) = self.segments.get(seg) {
            let c = c.min(s.rope.len_chars());
            self.cursor.1 = s.rope.line_to_char(s.rope.char_to_line(c));
        }
    }

    pub fn move_end(&mut self) {
        let (seg, c) = self.cursor;
        if let Some(s) = self.segments.get(seg) {
            let c = c.min(s.rope.len_chars());
            let line = s.rope.char_to_line(c);
            let start = s.rope.line_to_char(line);
            let mut end = start + s.rope.line(line).len_chars();
            if end > start && s.rope.get_char(end - 1) == Some('\n') {
                end -= 1;
            }
            self.cursor.1 = end.min(s.rope.len_chars());
        }
    }

    pub fn delete_current_line(&mut self) {
        let (seg, c) = self.cursor;
        let Some(s) = self.segments.get_mut(seg) else {
            return;
        };
        let c = c.min(s.rope.len_chars());
        let line = s.rope.char_to_line(c);
        let start = s.rope.line_to_char(line);
        let len = s.rope.line(line).len_chars();
        if len == 0 {
            return;
        }
        let text: String = s.rope.slice(start..start + len).chars().collect();
        s.history.record_delete(start, text, false);
        s.rope.remove(start..start + len);
        s.dirty = true;
        self.cursor.1 = start.min(s.rope.len_chars());
    }

    /// Undo one step in the active segment. Never crosses segment
    /// boundaries: each file keeps its own history.
    pub fn undo(&mut self) -> bool {
        let (seg, _) = self.cursor;
        let Some(s) = self.segments.get_mut(seg) else {
            return false;
        };
        let mut cur = self.cursor.1.min(s.rope.len_chars());
        if s.history.undo(&mut s.rope, &mut cur) {
            s.dirty = true;
            self.cursor.1 = cur.min(s.rope.len_chars());
            true
        } else {
            false
        }
    }

    /// Redo one undone step in the active segment.
    pub fn redo(&mut self) -> bool {
        let (seg, _) = self.cursor;
        let Some(s) = self.segments.get_mut(seg) else {
            return false;
        };
        let mut cur = self.cursor.1.min(s.rope.len_chars());
        if s.history.redo(&mut s.rope, &mut cur) {
            s.dirty = true;
            self.cursor.1 = cur.min(s.rope.len_chars());
            true
        } else {
            false
        }
    }
}

/// Wrapped visual-row height of one flattened multi-view row (for wheel
/// scrolling): dividers are always a single row.
fn mv_flat_row_height(mv: &MultiView, flat: usize, width: u16) -> usize {
    match mv.flat_at(flat) {
        Some((_, None)) => 1,
        Some((seg, Some(line))) => {
            let rope = &mv.segments[seg].rope;
            if line >= rope.len_lines() {
                return 1;
            }
            let owned = rope.line(line).to_string();
            let t = crate::view::strip_newline(&owned);
            crate::view::wrap_geometry(t, width).len().max(1)
        }
        None => 0,
    }
}

/// Ctrl+Left word jump inside the active multi-view segment.
fn word_jump_left_mv(mv: &mut MultiView) {
    use crate::words::is_word_char;
    // Pass 1: skip non-word chars.
    loop {
        let (seg, c) = mv.cursor;
        let len = mv.segments.get(seg).map(|s| s.rope.len_chars()).unwrap_or(0);
        let c = c.min(len);
        if c == 0 {
            if seg == 0 {
                break;
            }
            mv.cursor = (seg - 1, mv.segments[seg - 1].rope.len_chars());
            continue;
        }
        if is_word_char(mv.segments[seg].rope.get_char(c - 1).unwrap_or(' ')) {
            break;
        }
        mv.cursor.1 = c - 1;
    }
    // Pass 2: skip word chars.
    loop {
        let (seg, c) = mv.cursor;
        let len = mv.segments.get(seg).map(|s| s.rope.len_chars()).unwrap_or(0);
        let c = c.min(len);
        if c == 0 {
            if seg == 0 {
                break;
            }
            mv.cursor = (seg - 1, mv.segments[seg - 1].rope.len_chars());
            continue;
        }
        if !is_word_char(mv.segments[seg].rope.get_char(c - 1).unwrap_or(' ')) {
            break;
        }
        mv.cursor.1 = c - 1;
    }
}

/// Which pane separator is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    Library,
    Sheets,
}

/// In-progress pane-separator drag: rendered widths at grab time plus the
/// main-area geometry, so drops clamp against live minimums.
#[derive(Debug, Clone, Copy)]
pub struct DragState {
    pub sep: Separator,
    pub main_x: u16,
    pub main_w: u16,
    pub lib_w: u16,
    pub sheets_w: u16,
    pub orig_lib: f32,
    pub orig_sheets: f32,
}

/// Minimum side-pane width in columns while dragging.
pub const MIN_PANE_W: u16 = 12;
/// Minimum editor width in columns while dragging.
pub const EDITOR_MIN_W: u16 = 30;

pub struct App {
    pub library_root: PathBuf,
    pub data_dir: PathBuf,
    pub library: Library,
    pub focus: Focus,
    pub lib_cursor: usize,
    pub sheet_cursor: usize,
    pub lib_scroll: usize,
    pub sheet_scroll: usize,
    /// Open-buffer word counts need a library refresh (debounced, see C).
    pub stats_dirty: bool,
    pub last_stats_sync: Option<Instant>,
    pub editor: Option<SheetBuffer>,
    pub editor_words: usize,
    pub editor_scroll: u16,
    /// First rope line in the editor render window; `editor_scroll` counts
    /// visual (wrapped) rows down from here. Keeps per-frame work O(window).
    pub editor_anchor: usize,
    /// Last rendered cursor (single-buffer char index), to tell keyboard
    /// movement apart from wheel scrolling: the viewport only snaps back to
    /// the cursor when the cursor itself moved.
    pub last_cursor_single: Option<usize>,
    /// Same for multi-view, as (segment, char index).
    pub last_cursor_mv: Option<(usize, usize)>,
    /// Displayed visual rows for click mapping, rebuilt every render.
    pub editor_map: Vec<EditorMapRow>,
    /// Same, for the multi-view editor (segments flattened with dividers).
    pub mv_map: Vec<MvMapRow>,
    pub stats: Daily,
    pub status: String,
    pub should_quit: bool,
    pub last_edit: Option<Instant>,
    pub open_menu: Option<MenuKind>,
    pub menu_cursor: usize,
    pub show_help: bool,
    pub show_guide: bool,
    pub new_prompt: Option<NewSheetPrompt>,
    pub group_prompt: Option<GroupPrompt>,
    pub search_prompt: Option<SearchPrompt>,
    /// (query, match start) of the last jump: the next Enter continues
    /// strictly after it; any other cursor position searches from the
    /// cursor instead. Cleared whenever the query text changes.
    pub search_last: Option<(String, usize)>,
    /// Pending two-step group delete: press the delete key again to confirm.
    pub pending_group_delete: Option<Vec<PathBuf>>,
    /// Pending two-step sheet delete (Sheets pane or File menu scope).
    pub pending_sheet_delete: Option<Vec<PathBuf>>,
    /// Marked groups (Library `X`) for batch move/delete.
    pub selected_groups: HashSet<PathBuf>,
    /// Marked sheets, absolute paths (Sheets `X`). 2+ drives multi-view.
    pub selected_sheets: HashSet<PathBuf>,
    /// Lifted items awaiting drop (`M` move mode).
    pub move_lift: Option<MoveLift>,
    /// Sequential multi-sheet editor (active when 2+ sheets marked).
    pub multiview: Option<MultiView>,
    /// Sheet rename target (abs path); `None` means the prompt creates.
    pub sheet_rename_target: Option<PathBuf>,
    /// Single zen toggle for both side panels (Ctrl+P). Persisted.
    pub show_panels: bool,
    /// Fraction of main width for the Library pane (editor takes the rest).
    pub lib_frac: f32,
    /// Fraction of main width for the Sheets pane.
    pub sheets_frac: f32,
    /// Active pane-separator drag (mouse). `None` when not dragging.
    pub drag: Option<DragState>,
    /// Main content rect (panels + editor area) recorded in ui.rs.
    pub main_area: Option<Rect>,
    /// Mouse support (click + wheel). On by default, persisted.
    pub mouse_enabled: bool,
    // --- last-frame geometry for mouse hit-testing (recorded in ui.rs) ---
    pub lib_area: Option<Rect>,
    pub sheets_area: Option<Rect>,
    pub editor_area: Option<Rect>,
    pub editor_inner: Option<Rect>,
    pub menu_area: Option<Rect>,
    /// (terminal row, visible-group index) for clickable Library rows.
    pub lib_rows: Vec<(u16, usize)>,
    /// (terminal row, sheet-list index) for clickable Sheets rows.
    pub sheet_rows: Vec<(u16, usize)>,
    /// Top-bar File/Edit/View hitboxes (menu kind + rect), recorded in ui.rs.
    pub menu_bar: Vec<(MenuKind, Rect)>,
    // --- export (CSS-driven, mirrors Ariadne) ---
    pub export_css: String,
    pub export_css_name: String,
    pub export_format: crate::export::ExportFormat,
    pub attached_fonts: Vec<String>,
    pub show_export: bool,
    pub export_col: usize,
    pub export_style_cursor: usize,
    pub export_format_cursor: usize,
    pub export_output: String,
    pub export_preview: bool,
    pub export_prompt: Option<ExportPrompt>,
    pub export_confirm_overwrite: Option<PathBuf>,
    /// Live external preview file for PDF/ePub (zathura, Linux-only).
    /// `Some` while a viewer has been launched; the file is reused in place
    /// so the viewer auto-reloads. Kept after dialog close for cleanup on quit.
    pub export_zathura_path: Option<PathBuf>,
    /// Handle to the launched viewer, used to detect whether it is still
    /// alive (closed viewers are relaunched on the next preview attempt).
    pub export_zathura_child: Option<std::process::Child>,
}

pub const EDITOR_MAX_WIDTH: u16 = 80;

#[derive(serde::Serialize, serde::Deserialize)]
struct UiState {
    #[serde(default = "default_true")]
    show_panels: bool,
    #[serde(default = "default_true")]
    mouse_enabled: bool,
    #[serde(default = "default_quarter")]
    lib_frac: f32,
    #[serde(default = "default_quarter")]
    sheets_frac: f32,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            show_panels: true,
            mouse_enabled: true,
            lib_frac: 0.25,
            sheets_frac: 0.25,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_quarter() -> f32 {
    0.25
}

impl UiState {
    fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("state.json")
    }

    fn load(data_dir: &Path) -> Self {
        std::fs::read(Self::path(data_dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, data_dir: &Path) {
        let _ = std::fs::create_dir_all(data_dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(data_dir), s);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct ExportState {
    #[serde(default)]
    css_name: String,
    #[serde(default)]
    format: String,
    #[serde(default)]
    attached_fonts: Vec<String>,
}

impl ExportState {
    fn path(data_dir: &Path) -> PathBuf {
        data_dir.join("export.json")
    }

    fn load(data_dir: &Path) -> Self {
        std::fs::read(Self::path(data_dir))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn save(&self, data_dir: &Path) {
        let _ = std::fs::create_dir_all(data_dir);
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(data_dir), s);
        }
    }
}

impl App {
    pub fn new(library_root: PathBuf, data_dir: PathBuf) -> Self {
        let _ = config::ensure_dirs(&library_root, &data_dir);
        let library = Library::scan(&library_root);
        let total = library.total_words;
        let stats = Daily::load(&data_dir, total);
        let ui_state = UiState::load(&data_dir);
        let export_state = ExportState::load(&data_dir);
        let export_format = crate::export::ExportFormat::from_extension(&export_state.format).unwrap_or_default();
        let (export_css, export_css_name) = if export_state.css_name.is_empty() {
            (String::new(), "Built-in".to_string())
        } else {
            crate::export::css_for_style(&data_dir, &export_state.css_name)
        };
        let mut app = Self {
            library_root,
            data_dir,
            library,
            focus: Focus::Editor,
            lib_cursor: 0,
            sheet_cursor: 0,
            lib_scroll: 0,
            sheet_scroll: 0,
            stats_dirty: false,
            last_stats_sync: None,
            editor: None,
            editor_words: 0,
            editor_scroll: 0,
            editor_anchor: 0,
            last_cursor_single: None,
            last_cursor_mv: None,
            editor_map: Vec::new(),
            mv_map: Vec::new(),
            stats,
            status: String::new(),
            should_quit: false,
            last_edit: None,
            open_menu: None,
            menu_cursor: 0,
            show_help: false,
            show_guide: false,
            new_prompt: None,
            group_prompt: None,
            search_prompt: None,
            search_last: None,
            pending_group_delete: None,
            pending_sheet_delete: None,
            selected_groups: HashSet::new(),
            selected_sheets: HashSet::new(),
            move_lift: None,
            multiview: None,
            sheet_rename_target: None,
            show_panels: ui_state.show_panels,
            mouse_enabled: ui_state.mouse_enabled,
            lib_frac: ui_state.lib_frac,
            sheets_frac: ui_state.sheets_frac,
            drag: None,
            main_area: None,
            lib_area: None,
            sheets_area: None,
            editor_area: None,
            editor_inner: None,
            menu_area: None,
            lib_rows: Vec::new(),
            sheet_rows: Vec::new(),
            menu_bar: Vec::new(),
            export_css,
            export_css_name,
            export_format,
            attached_fonts: export_state.attached_fonts,
            show_export: false,
            export_col: 0,
            export_style_cursor: 0,
            export_format_cursor: 0,
            export_output: String::new(),
            export_preview: false,
            export_prompt: None,
            export_confirm_overwrite: None,
            export_zathura_path: None,
            export_zathura_child: None,
        };
        app.auto_select_first();
        app
    }

    fn auto_select_first(&mut self) {
        // Prefer Writing/Book 1/Sheet3.md-like first sheet if any, else first group
        if !self.library.groups.is_empty() {
            self.lib_cursor = 0;
        }
        self.sheet_cursor = 0;
        // open first sheet of first group for parity with the mock
        let group = self.selected_group();
        let sheets = self.library.sheets_for_group(group.as_deref());
        if let Some(first) = sheets.first() {
            let abs = first.abs.clone();
            self.open_sheet(&abs);
            self.focus = Focus::Editor;
        } else if !self.library.sheets.is_empty() {
            let abs = self.library.sheets[0].abs.clone();
            self.open_sheet(&abs);
        }
    }

    pub fn selected_group(&self) -> Option<PathBuf> {
        let vis = self.library.visible_groups();
        if vis.is_empty() {
            return None;
        }
        let i = self.lib_cursor.min(vis.len().saturating_sub(1));
        Some(vis[i].rel.clone())
    }

    pub fn current_sheets(&self) -> Vec<std::path::PathBuf> {
        let g = self.selected_group();
        self.library
            .sheets_for_group(g.as_deref())
            .iter()
            .map(|s| s.abs.clone())
            .collect()
    }

    pub fn open_sheet(&mut self, abs: &Path) {
        match SheetBuffer::open(abs) {
            Ok(buf) => {
                self.editor_words = buf.words();
                self.editor = Some(buf);
                self.editor_scroll = 0;
                self.editor_anchor = 0;
                // Fresh open is already consistent (cursor 0 at the top),
                // so no forced recenter: wheel scrolling works immediately.
                self.last_cursor_single = Some(0);
                self.last_cursor_mv = None;
                self.editor_map.clear();
                self.mv_map.clear();
                self.multiview = None;
                self.stats_dirty = false;
                self.status.clear();
            }
            Err(e) => {
                self.status = format!("open failed: {e}");
            }
        }
    }

    pub fn open_selected_sheet(&mut self) {
        let g = self.selected_group();
        let sheets = self.library.sheets_for_group(g.as_deref());
        if sheets.is_empty() {
            self.status = "no sheets in group".to_string();
            return;
        }
        let i = self.sheet_cursor.min(sheets.len() - 1);
        let abs = sheets[i].abs.clone();
        // Explicit open wins over multi-view.
        self.selected_sheets.clear();
        self.selected_groups.clear();
        self.pending_sheet_delete = None;
        self.open_sheet(&abs);
        self.focus = Focus::Editor;
    }

    pub fn save(&mut self) {
        if self.multiview.is_some() {
            self.mv_save();
            return;
        }
        if let Some(ed) = self.editor.as_mut() {
            let wc = ed.words();
            let preview = words::sheet_preview_rope(&ed.rope);
            match ed.save() {
                Ok(()) => {
                    self.library.refresh_sheet(&ed.path.clone(), wc, preview);
                    self.editor_words = wc;
                    self.stats_dirty = false;
                    self.last_stats_sync = Some(Instant::now());
                    // persist daily in case baseline changed externally
                    let _ = self.stats.save(&self.data_dir);
                    self.status = "saved".to_string();
                    self.last_edit = None;
                    self.refresh_zathura_preview();
                }
                Err(e) => self.status = format!("save failed: {e}"),
            }
        }
    }

    /// Save all dirty multi-view segments, each atomically to its own file.
    pub fn mv_save(&mut self) {
        let Some(mv) = self.multiview.as_mut() else {
            return;
        };
        let mut failed = 0;
        let mut paths = Vec::new();
        for s in mv.segments.iter_mut() {
            if !s.dirty {
                continue;
            }
            match atomic_write(&s.path, &s.rope.to_string()) {
                Ok(()) => {
                    s.dirty = false;
                    paths.push(s.path.clone());
                }
                Err(_) => failed += 1,
            }
        }
        for abs in &paths {
            if let Some(seg) = mv.segments.iter().find(|s| &s.path == abs) {
                let wc = seg.words();
                let preview = words::sheet_preview_rope(&seg.rope);
                self.library.refresh_sheet(abs, wc, preview);
            }
        }
        self.editor_words = mv.words();
        self.stats_dirty = false;
        self.last_stats_sync = Some(Instant::now());
        let _ = self.stats.save(&self.data_dir);
        self.last_edit = None;
        self.status = if failed == 0 {
            self.refresh_zathura_preview();
            "saved".to_string()
        } else {
            format!("saved with {failed} failure(s)")
        };
    }

    pub fn maybe_autosave(&mut self) -> bool {
        if let Some(t) = self.last_edit {
            if t.elapsed().as_millis() >= 500 {
                let dirty = self
                    .editor
                    .as_ref()
                    .map(|e| e.dirty)
                    .unwrap_or(false)
                    || self.multiview.as_ref().map(|m| m.is_dirty()).unwrap_or(false);
                if dirty {
                    self.save();
                } else {
                    self.last_edit = None;
                }
                // Either path flips the statusbar (saved / ● editing).
                return true;
            }
        }
        false
    }

    pub fn on_typed_word(&mut self) {
        self.stats.record_words_completed(1, &self.data_dir);
        self.last_edit = Some(Instant::now());
        self.stats_dirty = true;
    }

    pub fn on_edit_no_word(&mut self) {
        self.last_edit = Some(Instant::now());
        self.stats_dirty = true;
    }

    /// Refresh open-buffer word counts into the library totals.
    /// Debounced: per-keystroke exactness isn't needed for the statusbar,
    /// so this runs on the main-loop tick (and forced on save/switch).
    /// Returns true when a refresh actually ran (caller may need a redraw).
    pub fn maybe_refresh_stats(&mut self, force: bool) -> bool {
        if !self.stats_dirty {
            return false;
        }
        if !force {
            // At most one full recount per throttle window while typing.
            const STATS_MIN_INTERVAL_MS: u128 = 300;
            if let Some(last) = self.last_stats_sync {
                if last.elapsed().as_millis() < STATS_MIN_INTERVAL_MS {
                    return false;
                }
            }
        }
        self.sync_words_cache();
        self.stats_dirty = false;
        self.last_stats_sync = Some(Instant::now());
        true
    }

    fn sync_words_cache(&mut self) {
        // keep total_words live without disk IO: update in-memory entry.
        // Previews come from a rope prefix (no full-buffer copy).
        if let Some(mv) = self.multiview.as_ref() {
            let words: Vec<(PathBuf, usize, String)> = mv
                .segments
                .iter()
                .filter(|s| s.dirty)
                .map(|s| (s.path.clone(), s.words(), words::sheet_preview_rope(&s.rope)))
                .collect();
            for (abs, wc, preview) in words {
                self.library.refresh_sheet(&abs, wc, preview);
            }
            self.editor_words = mv.words();
            return;
        }
        if let Some(ed) = self.editor.as_ref() {
            let wc = ed.words();
            let preview = words::sheet_preview_rope(&ed.rope);
            // avoid borrowing issues
            let abs = ed.path.clone();
            self.library.refresh_sheet(&abs, wc, preview);
            self.editor_words = wc;
        }
    }

    // --- editor dispatch: single buffer or the active multi-view segment ---

    pub fn ed_insert_char(&mut self, c: char) {
        let done = if let Some(mv) = self.multiview.as_mut() {
            mv.insert_char(c)
        } else if let Some(ed) = self.editor.as_mut() {
            ed.insert_char(c)
        } else {
            return;
        };
        if done {
            self.on_typed_word();
        } else {
            self.on_edit_no_word();
        }
    }

    pub fn ed_insert_newline(&mut self) {
        let done = if let Some(mv) = self.multiview.as_mut() {
            mv.insert_newline()
        } else if let Some(ed) = self.editor.as_mut() {
            ed.insert_newline()
        } else {
            return;
        };
        if done {
            self.on_typed_word();
        } else {
            self.on_edit_no_word();
        }
    }

    pub fn ed_backspace(&mut self) {
        if let Some(mv) = self.multiview.as_mut() {
            mv.backspace();
        } else if let Some(ed) = self.editor.as_mut() {
            ed.backspace();
        } else {
            return;
        }
        self.on_edit_no_word();
    }

    pub fn ed_delete_forward(&mut self) {
        if let Some(mv) = self.multiview.as_mut() {
            mv.delete_forward();
        } else if let Some(ed) = self.editor.as_mut() {
            ed.delete_forward();
        } else {
            return;
        }
        self.on_edit_no_word();
    }

    pub fn ed_delete_line(&mut self) {
        if let Some(mv) = self.multiview.as_mut() {
            mv.delete_current_line();
        } else if let Some(ed) = self.editor.as_mut() {
            ed.delete_current_line();
        } else {
            return;
        }
        self.on_edit_no_word();
    }

    pub fn ed_move_up(&mut self) {
        let w = self.editor_inner.map(|r| r.width).unwrap_or(0);
        if let Some(mv) = self.multiview.as_mut() {
            mv.move_up_visual(w);
        } else if let Some(ed) = self.editor.as_mut() {
            ed.move_up_visual(w);
        }
    }

    pub fn ed_move_down(&mut self) {
        let w = self.editor_inner.map(|r| r.width).unwrap_or(0);
        if let Some(mv) = self.multiview.as_mut() {
            mv.move_down_visual(w);
        } else if let Some(ed) = self.editor.as_mut() {
            ed.move_down_visual(w);
        }
    }

    /// Undo one editing step in the open buffer (or active multi-view
    /// segment). Histories are per file and reset when files are (re)opened.
    pub fn ed_undo(&mut self) {
        let done = if let Some(mv) = self.multiview.as_mut() {
            mv.undo()
        } else if let Some(ed) = self.editor.as_mut() {
            ed.undo()
        } else {
            false
        };
        if done {
            self.on_edit_no_word();
        } else {
            self.status = "nothing to undo".to_string();
        }
    }

    /// Redo one undone step.
    pub fn ed_redo(&mut self) {
        let done = if let Some(mv) = self.multiview.as_mut() {
            mv.redo()
        } else if let Some(ed) = self.editor.as_mut() {
            ed.redo()
        } else {
            false
        };
        if done {
            self.on_edit_no_word();
        } else {
            self.status = "nothing to redo".to_string();
        }
    }

    pub fn ed_move_left(&mut self, word_jump: bool) {
        if let Some(mv) = self.multiview.as_mut() {
            if word_jump {
                word_jump_left_mv(mv);
            } else {
                mv.move_left();
            }
            return;
        }
        if let Some(ed) = self.editor.as_mut() {
            if word_jump {
                // word jump: skip non-word then word
                while ed.cursor > 0
                    && !crate::words::is_word_char(ed.rope.get_char(ed.cursor - 1).unwrap_or(' '))
                {
                    ed.move_left();
                }
                while ed.cursor > 0
                    && crate::words::is_word_char(ed.rope.get_char(ed.cursor - 1).unwrap_or(' '))
                {
                    ed.move_left();
                }
            } else {
                ed.move_left();
            }
        }
    }

    pub fn ed_move_right(&mut self) {
        if let Some(mv) = self.multiview.as_mut() {
            mv.move_right();
        } else if let Some(ed) = self.editor.as_mut() {
            ed.move_right();
        }
    }

    pub fn ed_move_home(&mut self) {
        if let Some(mv) = self.multiview.as_mut() {
            mv.move_home();
        } else if let Some(ed) = self.editor.as_mut() {
            ed.move_home();
        }
    }

    pub fn ed_move_end(&mut self) {
        if let Some(mv) = self.multiview.as_mut() {
            mv.move_end();
        } else if let Some(ed) = self.editor.as_mut() {
            ed.move_end();
        }
    }

    /// Mouse-wheel vertical scroll of the editor viewport. Down extends the
    /// window forward (the render pass grows it and trims the top); up moves
    /// the anchor when the scroll offset is exhausted.
    pub fn ed_wheel(&mut self, up: bool) {
        if self.multiview.is_some() {
            self.mv_wheel(up);
            return;
        }
        let width = self.editor_inner.map(|r| r.width).unwrap_or(0);
        if !up {
            self.editor_scroll = self.editor_scroll.saturating_add(3);
            return;
        }
        if width == 0 || self.editor_scroll >= 3 {
            self.editor_scroll = self.editor_scroll.saturating_sub(3);
            return;
        }
        let Some(ed) = self.editor.as_ref() else {
            self.editor_scroll = 0;
            return;
        };
        // Walk rope lines above the anchor, wrapping until the viewport can
        // move up 3 visual rows (or BOF).
        let n = ed.rope.len_lines().max(1);
        let mut anchor = self.editor_anchor.min(n - 1);
        let mut rows = 0usize;
        let need = 3usize.saturating_sub(self.editor_scroll as usize);
        while anchor > 0 && rows < need {
            anchor -= 1;
            let owned = ed.rope.line(anchor).to_string();
            let t = crate::view::strip_newline(&owned);
            rows += crate::view::wrap_geometry(t, width).len().max(1);
        }
        let new_scroll = (rows + self.editor_scroll as usize).saturating_sub(3);
        self.editor_anchor = anchor;
        self.editor_scroll = new_scroll.min(u16::MAX as usize) as u16;
    }

    /// Multi-view wheel scroll over the flattened segment/​divider rows.
    pub fn mv_wheel(&mut self, up: bool) {
        let Some(mv) = self.multiview.as_ref() else {
            return;
        };
        if !up {
            if let Some(mv) = self.multiview.as_mut() {
                mv.scroll = mv.scroll.saturating_add(3);
            }
            return;
        }
        let width = self.editor_inner.map(|r| r.width).unwrap_or(0);
        let (mut anchor, mut scroll) = (mv.anchor, mv.scroll);
        let total = mv.flat_len();
        if total == 0 {
            return;
        }
        anchor = anchor.min(total - 1);
        if width == 0 || scroll >= 3 {
            if let Some(mv) = self.multiview.as_mut() {
                mv.scroll = scroll.saturating_sub(3);
                mv.anchor = anchor;
            }
            return;
        }
        let need = 3usize.saturating_sub(scroll as usize);
        let mut rows = 0usize;
        while anchor > 0 && rows < need {
            anchor -= 1;
            rows += mv_flat_row_height(self.multiview.as_ref().unwrap(), anchor, width);
        }
        scroll = (rows + scroll as usize).saturating_sub(3) as u16;
        if let Some(mv) = self.multiview.as_mut() {
            mv.anchor = anchor;
            mv.scroll = scroll;
        }
    }

    pub fn ed_is_dirty(&self) -> bool {
        self.editor.as_ref().map(|e| e.dirty).unwrap_or(false)
            || self.multiview.as_ref().map(|m| m.is_dirty()).unwrap_or(false)
    }

    /// Transitive label for the status bar: transient status, else saved state.
    /// Marked counts are appended persistently (autosave can't wipe them).
    pub fn status_label(&self) -> String {
        let base = if self.status.is_empty() {
            self.saved_label().to_string()
        } else {
            self.status.clone()
        };
        let n = self.selected_groups.len() + self.selected_sheets.len();
        if n > 0 {
            format!("{base}  |  {n} selected")
        } else {
            base
        }
    }

    pub fn display_path(&self) -> String {
        if self.multiview.is_some() {
            let n = self.multiview.as_ref().map(|m| m.segments.len()).unwrap_or(0);
            return format!("{n} sheets");
        }
        Self::escape_path(&self.editor_path_or_root().to_string_lossy())
    }

    /// Path relative to the library root (`Writing/Book 1/Sheet3.md`),
    /// falling back to the full display path when the sheet is outside it.
    pub fn rel_display_path(&self) -> String {
        if self.multiview.is_some() {
            return self.display_path();
        }
        if let Some(ed) = self.editor.as_ref() {
            if let Ok(rel) = ed.path.strip_prefix(&self.library_root) {
                return Self::escape_path(&rel.to_string_lossy());
            }
        }
        self.display_path()
    }

    /// Sheet file name only (`Sheet3.md`), or the library root when empty.
    pub fn file_display_name(&self) -> String {
        if self.multiview.is_some() {
            return self.display_path();
        }
        if let Some(ed) = self.editor.as_ref() {
            if let Some(name) = ed.path.file_name().and_then(|s| s.to_str()) {
                return Self::escape_path(name);
            }
        }
        self.display_path()
    }

    fn editor_path_or_root(&self) -> PathBuf {
        if let Some(ed) = self.editor.as_ref() {
            if let Ok(home) = std::env::var("HOME") {
                let s = ed.path.to_string_lossy().to_string();
                if s.starts_with(&home) {
                    return PathBuf::from(format!("~{}", &s[home.len()..]));
                }
            }
            return ed.path.clone();
        }
        self.library_root.clone()
    }

    fn escape_path(s: &str) -> String {
        s.replace(' ', "\\ ")
    }

    pub fn saved_label(&self) -> &str {
        if self.ed_is_dirty() {
            "● editing"
        } else {
            "saved"
        }
    }

    pub fn toggle_menu(&mut self, kind: MenuKind) {
        if self.open_menu == Some(kind) {
            self.open_menu = None;
        } else {
            self.open_menu = Some(kind);
            self.menu_cursor = 0;
        }
        self.show_help = false;
        self.show_guide = false;
    }

    pub fn close_menu(&mut self) {
        self.open_menu = None;
        self.menu_cursor = 0;
    }

    /// (label, accelerator-letter) for the open menu. The accelerator is
    /// rendered bold+underlined and can be pressed directly (with or
    /// without Ctrl) while the menu is open.
    pub fn menu_items(&self) -> Vec<(String, char)> {
        match self.open_menu {
            Some(MenuKind::File) => vec![
                ("New sheet".to_string(), 'n'),
                ("New group".to_string(), 'g'),
                ("Save".to_string(), 's'),
                ("Delete current sheet".to_string(), 'd'),
                ("Quit".to_string(), 'q'),
            ],
            Some(MenuKind::Edit) => vec![
                ("Focus editor".to_string(), 'e'),
                ("Delete current line".to_string(), 'k'),
                ("Find in sheet".to_string(), 'f'),
                ("Undo".to_string(), 'z'),
                ("Redo".to_string(), 'y'),
            ],
            Some(MenuKind::View) => vec![
                ("Library".to_string(), '1'),
                ("Sheets".to_string(), '2'),
                ("Editor".to_string(), '3'),
                ("Toggle panels".to_string(), 'p'),
                ("Markdown guide".to_string(), 'm'),
                (
                    format!("Mouse: {}", if self.mouse_enabled { "on" } else { "off" }),
                    'o',
                ),
                ("Help".to_string(), 'h'),
            ],
            Some(MenuKind::Export) => vec![
                ("Save export".to_string(), 's'),
                ("Preview".to_string(), 'p'),
                ("Open with app".to_string(), 'o'),
                ("Next style".to_string(), 'n'),
                ("Next format".to_string(), 'f'),
                ("Manage styles".to_string(), 'm'),
            ],
            None => vec![],
        }
    }

    pub fn toggle_panels(&mut self) {
        self.show_panels = !self.show_panels;
        self.persist_ui();
        if !self.show_panels
            && (self.focus == Focus::Library || self.focus == Focus::Sheets)
        {
            self.focus = Focus::Editor;
        }
        self.status = if self.show_panels {
            "panels shown".to_string()
        } else {
            "panels hidden — ^P to restore".to_string()
        };
    }

    fn persist_ui(&self) {
        UiState {
            show_panels: self.show_panels,
            mouse_enabled: self.mouse_enabled,
            lib_frac: self.lib_frac,
            sheets_frac: self.sheets_frac,
        }
        .save(&self.data_dir);
    }

    /// Toggle mouse support. Returns the new state.
    pub fn toggle_mouse(&mut self) -> bool {
        self.mouse_enabled = !self.mouse_enabled;
        self.persist_ui();
        self.status = if self.mouse_enabled {
            "mouse on — click selects, wheel scrolls".to_string()
        } else {
            "mouse off".to_string()
        };
        self.mouse_enabled
    }

    pub fn apply_mouse_override(&mut self, on: bool) {
        if self.mouse_enabled != on {
            self.mouse_enabled = on;
            self.persist_ui();
        }
    }

    // --- export (CSS-driven, mirrors Ariadne) ---

    fn persist_export(&self) {
        ExportState {
            css_name: self.export_css_name.clone(),
            format: self.export_format.extension().to_string(),
            attached_fonts: self.attached_fonts.clone(),
        }
        .save(&self.data_dir);
    }

    fn refresh_export_css(&mut self) {
        let (css, name) = crate::export::css_for_style(&self.data_dir, &self.export_css_name);
        self.export_css = css;
        self.export_css_name = name;
    }

    pub fn open_export(&mut self) {
        self.close_menu();
        self.show_help = false;
        self.show_guide = false;
        self.refresh_export_css();
        // Seed cursors from current state.
        let styles = crate::export::all_style_names(&self.data_dir, &self.export_css_name);
        self.export_style_cursor = styles.iter().position(|s| s == &self.export_css_name).unwrap_or(0);
        let order = crate::export::ExportFormat::grid_order();
        self.export_format_cursor = order.iter().position(|f| *f == self.export_format).unwrap_or(0);
        if self.export_output.is_empty() {
            self.export_output = self.default_export_path();
        }
        self.show_export = true;
        self.export_preview = false;
        self.export_col = 0;
    }

    pub fn close_export(&mut self) {
        self.show_export = false;
        self.export_preview = false;
        self.export_prompt = None;
        self.export_confirm_overwrite = None;
        // Viewer keeps running; tmp file stays for cleanup on quit.
    }

    /// Whether this build can launch zathura (Linux-only for now).
    fn zathura_supported() -> bool {
        cfg!(target_os = "linux")
    }

    fn zathura_available() -> bool {
        if !Self::zathura_supported() {
            return false;
        }
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let cand = dir.join("zathura");
                cand.is_file()
            })
        })
    }

    /// One stable tmp path per format, reused so the viewer live-reloads.
    fn zathura_tmp_path(fmt: crate::export::ExportFormat) -> PathBuf {
        std::env::temp_dir().join(format!("alcott-preview-{}.{}", std::process::id(), fmt.extension()))
    }

    /// True while the launched viewer process is still running.
    /// A reaped/exited handle (or none) means the next preview must relaunch.
    fn zathura_alive(child: &mut Option<std::process::Child>) -> bool {
        match child {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Launch (or refresh) the live zathura preview for PDF/ePub.
    /// Returns true when the external viewer is active (overlay skipped).
    /// Missing binary / spawn failure falls back to the in-TUI overlay.
    pub fn preview_pdf_epub_zathura(&mut self) -> bool {
        use crate::export::ExportFormat;
        if !matches!(self.export_format, ExportFormat::Pdf | ExportFormat::Epub) {
            return false;
        }
        if !Self::zathura_available() {
            self.export_preview = true;
            self.status = "zathura not found — showing text preview".to_string();
            return false;
        }
        let docs = self.export_docs();
        if docs.is_empty() {
            self.status = "nothing to export".to_string();
            return false;
        }
        let (bytes, warnings) = self.export_bytes_for_docs(&docs);
        if bytes.is_empty() {
            let mut s = "export failed (no bytes)".to_string();
            Self::append_warnings(&mut s, &warnings);
            self.status = s;
            return false;
        }
        let path = Self::zathura_tmp_path(self.export_format);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::write(&path, &bytes) {
            self.status = format!("preview write failed: {e}");
            self.export_preview = true;
            return false;
        }
        // Remove a stale tmp file for the other format so only one lives.
        let other = match self.export_format {
            ExportFormat::Pdf => Self::zathura_tmp_path(ExportFormat::Epub),
            ExportFormat::Epub => Self::zathura_tmp_path(ExportFormat::Pdf),
            _ => unreachable!(),
        };
        if other != path {
            let _ = std::fs::remove_file(&other);
        }
        // Already previewing this exact file with a live viewer: just
        // refresh it so the open viewer reloads instead of opening a
        // second window. A closed viewer falls through to relaunch below.
        if self.export_zathura_path.as_ref() == Some(&path)
            && Self::zathura_alive(&mut self.export_zathura_child)
        {
            self.export_preview = false;
            let mut s = format!("refreshed in zathura: {}", path.display());
            Self::append_warnings(&mut s, &warnings);
            self.status = s;
            return true;
        }
        // Spawned without --fork so the Child handle stays valid while the
        // window is open (spawn itself never blocks). Stdio is nulled so
        // the GUI app doesn't inherit the TUI's terminal.
        match std::process::Command::new("zathura")
            .arg(&path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                self.export_zathura_child = Some(child);
                self.export_zathura_path = Some(path.clone());
                self.export_preview = false;
                let mut s = format!("previewing in zathura: {}", path.display());
                Self::append_warnings(&mut s, &warnings);
                self.status = s;
                true
            }
            Err(e) => {
                self.status = format!("zathura launch failed ({e}) — showing text preview");
                self.export_preview = true;
                false
            }
        }
    }

    /// Quick preview without the export dialog: same behavior as the
    /// dialog's `P` key. PDF/ePub goes to the external live viewer
    /// (zathura) when available; anything else toggles the in-TUI text
    /// overlay (`Esc` dismisses it while the dialog stays closed).
    pub fn toggle_preview(&mut self) {
        if matches!(
            self.export_format,
            crate::export::ExportFormat::Pdf | crate::export::ExportFormat::Epub
        ) {
            // External viewer replaces the overlay. A missing or
            // unlaunchable zathura falls back to the overlay with a
            // status note (handled inside).
            self.preview_pdf_epub_zathura();
        } else {
            self.export_preview = !self.export_preview;
        }
    }

    /// Rewrite the live preview file after style/format changes and after
    /// saves, so an open zathura viewer reloads in place. Runs whenever a
    /// preview is active (`export_zathura_path` set) — the viewer keeps
    /// running after the export dialog closes — and drops stale state if
    /// the viewer was closed meanwhile.
    fn refresh_zathura_preview(&mut self) {
        let Some(active) = self.export_zathura_path.clone() else {
            return;
        };
        if self.export_zathura_child.is_some()
            && !Self::zathura_alive(&mut self.export_zathura_child)
        {
            self.export_zathura_path = None;
            self.export_zathura_child = None;
            return;
        }
        use crate::export::ExportFormat;
        if !matches!(self.export_format, ExportFormat::Pdf | ExportFormat::Epub) {
            return;
        }
        let path = Self::zathura_tmp_path(self.export_format);
        let docs = self.export_docs();
        if docs.is_empty() {
            return;
        }
        let (bytes, _) = self.export_bytes_for_docs(&docs);
        if bytes.is_empty() {
            return;
        }
        if std::fs::write(&path, &bytes).is_ok() {
            if path != active {
                let _ = std::fs::remove_file(&active);
                self.export_zathura_path = Some(path);
            }
        }
    }

    /// Remove live preview tmp files. Called once on quit.
    pub fn cleanup_zathura_tmp(&mut self) {
        use crate::export::ExportFormat;
        for fmt in [ExportFormat::Pdf, ExportFormat::Epub] {
            let _ = std::fs::remove_file(Self::zathura_tmp_path(fmt));
        }
        if let Some(p) = self.export_zathura_path.take() {
            let _ = std::fs::remove_file(p);
        }
    }

    /// Documents currently targeted: marked sheets, else the open sheet(s).
    pub fn export_docs(&self) -> Vec<crate::export::SheetDoc> {
        let mut paths = self.open_sheet_paths();
        if paths.is_empty() {
            // Fall back to the sheet under the cursor.
            let g = self.selected_group();
            let sheets = self.library.sheets_for_group(g.as_deref());
            if let Some(s) = sheets.get(self.sheet_cursor.min(sheets.len().saturating_sub(1))) {
                paths.push(s.abs.clone());
            }
        }
        let mut docs = Vec::new();
        for p in paths {
            let content = if let Some(mv) = self.multiview.as_ref() {
                mv.segments.iter().find(|s| &s.path == &p).map(|s| s.rope.to_string())
            } else if let Some(ed) = self.editor.as_ref() {
                if ed.path == p {
                    Some(ed.rope.to_string())
                } else {
                    None
                }
            } else {
                None
            }
            .or_else(|| std::fs::read_to_string(&p).ok())
            .unwrap_or_default();
            let base = p.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| self.library_root.clone());
            let hint = p.file_stem().and_then(|s| s.to_str()).unwrap_or("sheet").to_string();
            docs.push(crate::export::SheetDoc::new(hint, content, base));
        }
        docs
    }

    fn default_export_path(&self) -> String {
        let docs = self.export_docs();
        let titles: Vec<String> = docs.iter().map(|d| crate::export::inferred_title(&d.content)).collect();
        let stem = crate::export::export_stem_for_titles(&titles);
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/Documents/Alcott/{stem}.{}", self.export_format.extension())
    }

    pub fn apply_export_style(&mut self, name: &str) {
        let (css, resolved) = crate::export::css_for_style(&self.data_dir, name);
        self.export_css = css;
        self.export_css_name = resolved.clone();
        let styles = crate::export::all_style_names(&self.data_dir, &resolved);
        self.export_style_cursor = styles.iter().position(|s| s == &resolved).unwrap_or(0);
        self.persist_export();
        self.status = format!("export style: {resolved}");
        self.refresh_zathura_preview();
    }

    pub fn apply_export_format(&mut self, fmt: crate::export::ExportFormat) {
        self.export_format = fmt;
        let order = crate::export::ExportFormat::grid_order();
        self.export_format_cursor = order.iter().position(|f| *f == fmt).unwrap_or(0);
        self.export_output = self.default_export_path();
        self.persist_export();
        self.refresh_zathura_preview();
    }

    fn export_bytes_for_docs(&self, docs: &[crate::export::SheetDoc]) -> (Vec<u8>, Vec<String>) {
        let mut warnings = Vec::new();
        // Re-read active style so outside edits apply (Ariadne parity).
        let css = crate::export::css_for_style(&self.data_dir, &self.export_css_name).0;
        if css != self.export_css {
            warnings.push("style reloaded from disk".to_string());
        }
        let titles: Vec<String> = docs.iter().map(|d| crate::export::inferred_title(&d.content)).collect();
        let stem = crate::export::export_stem_for_titles(&titles);
        let bytes: Vec<u8> = match self.export_format {
            crate::export::ExportFormat::Md => crate::export::export_markdown(docs).into_bytes(),
            crate::export::ExportFormat::Html => crate::export::export_html(docs, &css, &stem).into_bytes(),
            crate::export::ExportFormat::Rtf => {
                let (s, mut w) = crate::export::export_rtf(docs, &css, &self.attached_fonts);
                warnings.append(&mut w);
                s.into_bytes()
            }
            crate::export::ExportFormat::Docx => {
                let (b, mut w) = crate::export::export_docx_bytes(docs, &css, &self.attached_fonts);
                warnings.append(&mut w);
                b
            }
            crate::export::ExportFormat::Epub => crate::export::export_epub_bytes(docs, &css, &stem),
            crate::export::ExportFormat::Pdf => {
                let (b, mut w) = crate::export::export_pdf_bytes(docs, &css, &self.attached_fonts);
                warnings.append(&mut w);
                b
            }
        };
        (bytes, warnings)
    }

    fn append_warnings(status: &mut String, warnings: &[String]) {
        for w in warnings {
            status.push_str(" (");
            status.push_str(w);
            status.push(')');
        }
    }

    /// Save the current export. Never blind-overwrites: existing files get
    /// ` 2`, ` 3`, … unless a confirm is pending (second Enter overwrites).
    pub fn save_export(&mut self) {
        let docs = self.export_docs();
        if docs.is_empty() {
            self.status = "nothing to export".to_string();
            return;
        }
        if self.export_output.trim().is_empty() {
            self.export_output = self.default_export_path();
        }
        let requested = PathBuf::from(self.export_output.trim());
        // Conflict handling: existing file requires confirmation.
        if requested.exists() && self.export_confirm_overwrite.as_ref() != Some(&requested) {
            self.export_confirm_overwrite = Some(requested.clone());
            self.status = format!("exists: {} — Enter again to overwrite, or edit path", requested.display());
            return;
        }
        self.export_confirm_overwrite = None;
        let (bytes, warnings) = self.export_bytes_for_docs(&docs);
        if bytes.is_empty() {
            self.status = "export failed (no bytes)".to_string();
            Self::append_warnings(&mut self.status, &warnings);
            return;
        }
        if let Some(parent) = requested.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        match std::fs::write(&requested, &bytes) {
            Ok(()) => {
                let n = docs.len();
                let noun = if n == 1 { "sheet" } else { "sheets" };
                self.status = format!("exported {n} {noun} to {} ({})", requested.display(), crate::export::human_size(bytes.len()));
                Self::append_warnings(&mut self.status, &warnings);
                self.persist_export();
            }
            Err(e) => self.status = format!("export failed: {e}"),
        }
    }

    /// Save to a conflict-free sibling (`name 2.ext`, …) instead of asking.
    pub fn save_export_unique(&mut self) {
        let docs = self.export_docs();
        if docs.is_empty() {
            self.status = "nothing to export".to_string();
            return;
        }
        if self.export_output.trim().is_empty() {
            self.export_output = self.default_export_path();
        }
        let requested = PathBuf::from(self.export_output.trim());
        let target = crate::export::unique_output_path(&requested, false);
        if target != requested {
            self.status = format!("exists, writing {} instead", target.display());
            self.export_output = target.to_string_lossy().to_string();
        }
        self.export_confirm_overwrite = None;
        // Reuse save path but bypass the confirm gate by pre-clearing.
        let docs2 = self.export_docs();
        let (bytes, warnings) = self.export_bytes_for_docs(&docs2);
        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        match std::fs::write(&target, &bytes) {
            Ok(()) => {
                let n = docs2.len();
                let noun = if n == 1 { "sheet" } else { "sheets" };
                let mut s = format!("exported {n} {noun} to {} ({})", target.display(), crate::export::human_size(bytes.len()));
                Self::append_warnings(&mut s, &warnings);
                if self.status.starts_with("exists,") {
                    let prefix = std::mem::take(&mut self.status);
                    self.status = format!("{prefix} — {s}");
                } else {
                    self.status = s;
                }
                self.persist_export();
            }
            Err(e) => self.status = format!("export failed: {e}"),
        }
    }

    pub fn open_export_external(&mut self) {
        let path = PathBuf::from(self.export_output.trim());
        if !path.exists() {
            self.save_export();
        }
        let path = PathBuf::from(self.export_output.trim());
        if !path.exists() {
            return;
        }
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&path).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("cmd").args(["/C", "start", &path.to_string_lossy()]).spawn();
        self.status = format!("opened {}", path.display());
    }

    pub fn export_preview_text(&self, max_lines: usize) -> Vec<String> {
        let docs = self.export_docs();
        if docs.is_empty() {
            return vec!["nothing to export".to_string()];
        }
        let css = crate::export::css_for_style(&self.data_dir, &self.export_css_name).0;
        let titles: Vec<String> = docs.iter().map(|d| crate::export::inferred_title(&d.content)).collect();
        let stem = crate::export::export_stem_for_titles(&titles);
        match self.export_format {
            crate::export::ExportFormat::Md => crate::export::export_markdown(&docs).lines().take(max_lines).map(|s| s.to_string()).collect(),
            crate::export::ExportFormat::Html => crate::export::export_html(&docs, &css, &stem).lines().take(max_lines).map(|s| s.to_string()).collect(),
            crate::export::ExportFormat::Rtf => {
                let (rtf, warnings) = crate::export::export_rtf(&docs, &css, &self.attached_fonts);
                let mut v = vec![format!("RTF · {} sheet(s)", docs.len()), format!("style: {}", self.export_css_name)];
                for w in warnings {
                    v.push(format!("note: {w}"));
                }
                v.extend(rtf.lines().take(max_lines.saturating_sub(2)).map(|s| s.to_string()));
                v
            }
            crate::export::ExportFormat::Pdf | crate::export::ExportFormat::Docx | crate::export::ExportFormat::Epub => {
                let (bytes, warnings) = match self.export_format {
                    crate::export::ExportFormat::Pdf => crate::export::export_pdf_bytes(&docs, &css, &self.attached_fonts),
                    crate::export::ExportFormat::Docx => crate::export::export_docx_bytes(&docs, &css, &self.attached_fonts),
                    _ => (crate::export::export_epub_bytes(&docs, &css, &stem), Vec::new()),
                };
                let mut v = vec![
                    format!("{} · {} ({} sheets)", self.export_format.short(), crate::export::human_size(bytes.len()), docs.len()),
                    format!("style: {} · {}", self.export_css_name, stem),
                ];
                for w in warnings {
                    v.push(format!("note: {w}"));
                }
                if !self.attached_fonts.is_empty() {
                    v.push(format!("fonts: {} attached", self.attached_fonts.len()));
                }
                v.push(String::new());
                for doc in docs.iter().take(2) {
                    v.push(format!("— {} —", crate::export::inferred_title(&doc.content)));
                    for line in doc.content.lines().take(8) {
                        if v.len() >= max_lines {
                            break;
                        }
                        v.push(line.to_string());
                    }
                }
                v.truncate(max_lines);
                v
            }
        }
    }

    // --- style + font management (Ariadne parity) ---

    pub fn export_style_names(&self) -> Vec<String> {
        crate::export::all_style_names(&self.data_dir, &self.export_css_name)
    }

    pub fn create_export_style(&mut self, name: &str) {
        let clean = crate::export::sanitize_style_name(name);
        if clean.is_empty() {
            self.status = "style name is empty".to_string();
            return;
        }
        let mut final_name = clean.clone();
        if crate::export::is_reserved_style(&final_name) {
            final_name.push_str(" copy");
        }
        let dir = crate::export::styles_dir(&self.data_dir);
        let _ = std::fs::create_dir_all(&dir);
        let dest = crate::export::style_path_for(&dir, &final_name);
        let resolved: String = dest.file_stem().and_then(|s| s.to_str()).unwrap_or(&final_name).to_string();
        let template = if self.export_css.is_empty() { crate::export::STYLE_TEMPLATE.to_string() } else { self.export_css.clone() };
        match std::fs::write(&dest, &template) {
            Ok(()) => {
                self.export_css = template;
                self.export_css_name = resolved.clone();
                self.persist_export();
                self.status = format!("created style {resolved}");
            }
            Err(e) => self.status = format!("create style failed: {e}"),
        }
    }

    pub fn edit_export_style_as_copy(&mut self) {
        if self.export_css_name == "Built-in" || crate::export::virtual_style_css(&self.export_css_name).is_some() {
            let copy_name = format!("{} copy", self.export_css_name);
            let dir = crate::export::styles_dir(&self.data_dir);
            let _ = std::fs::create_dir_all(&dir);
            let dest = crate::export::style_path_for(&dir, &copy_name);
            let resolved: String = dest.file_stem().and_then(|s| s.to_str()).unwrap_or(&copy_name).to_string();
            // Virtual styles materialize from the active CSS.
            let css = if self.export_css.is_empty() {
                crate::export::virtual_style_css(&self.export_css_name).unwrap_or(crate::export::STYLE_TEMPLATE).to_string()
            } else {
                self.export_css.clone()
            };
            match std::fs::write(&dest, &css) {
                Ok(()) => {
                    self.export_css = css;
                    self.export_css_name = resolved.clone();
                    self.persist_export();
                    self.status = format!("editing copy {resolved} at {}", dest.display());
                }
                Err(e) => self.status = format!("copy failed: {e}"),
            }
        } else {
            let path = crate::export::styles_dir(&self.data_dir).join(format!("{}.css", self.export_css_name));
            self.status = format!("edit {} with an external editor, then re-save to apply", path.display());
        }
    }

    pub fn delete_export_style(&mut self, name: &str) {
        if crate::export::is_reserved_style(name) {
            self.status = "built-in styles can't be deleted".to_string();
            return;
        }
        let path = crate::export::styles_dir(&self.data_dir).join(format!("{name}.css"));
        match std::fs::remove_file(&path) {
            Ok(()) => {
                if self.export_css_name == name {
                    self.export_css.clear();
                    self.export_css_name = "Built-in".to_string();
                    self.persist_export();
                }
                self.status = format!("deleted style {name}");
            }
            Err(e) => self.status = format!("delete failed: {e}"),
        }
    }

    pub fn import_export_style(&mut self, src: &Path) {
        let stem = src.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "style".to_string());
        let mut name = crate::export::sanitize_style_name(&stem);
        if name.is_empty() {
            self.status = "that file name is not usable as a style".to_string();
            return;
        }
        if crate::export::is_reserved_style(&name) {
            name.push_str(" copy");
        }
        match std::fs::read_to_string(src) {
            Ok(css) => {
                let dir = crate::export::styles_dir(&self.data_dir);
                let _ = std::fs::create_dir_all(&dir);
                let dest = crate::export::style_path_for(&dir, &name);
                let resolved: String = dest.file_stem().and_then(|s| s.to_str()).unwrap_or(&name).to_string();
                match std::fs::write(&dest, &css) {
                    Ok(()) => {
                        self.export_css = css;
                        self.export_css_name = resolved.clone();
                        self.persist_export();
                        self.status = format!("imported style {resolved}");
                    }
                    Err(e) => self.status = format!("import failed: {e}"),
                }
            }
            Err(e) => self.status = format!("could not read style: {e}"),
        }
    }

    pub fn attach_export_font(&mut self, path: &Path) {
        let s = path.to_string_lossy().to_string();
        match crate::export::probe_font_file(&s) {
            Some((family, _)) => {
                if !self.attached_fonts.contains(&s) {
                    self.attached_fonts.push(s.clone());
                    self.persist_export();
                }
                self.status = format!("attached font {family} ({})", path.display());
            }
            None => self.status = format!("font unreadable: {}", path.display()),
        }
    }

    pub fn clear_export_fonts(&mut self) {
        self.attached_fonts.clear();
        self.persist_export();
        self.status = "attached fonts cleared".to_string();
    }

    // --- mouse hit-testing ---

    pub fn rect_contains(area: &Rect, x: u16, y: u16) -> bool {
        x >= area.x
            && x < area.x.saturating_add(area.width)
            && y >= area.y
            && y < area.y.saturating_add(area.height)
    }

    pub fn clear_pane_rects(&mut self) {
        self.lib_area = None;
        self.sheets_area = None;
        self.editor_area = None;
        self.editor_inner = None;
        self.lib_rows.clear();
        self.sheet_rows.clear();
    }

    /// Which pane (if any) owns this terminal point.
    pub fn pane_at(&self, x: u16, y: u16) -> Option<Focus> {
        if let Some(a) = self.lib_area {
            if Self::rect_contains(&a, x, y) {
                return Some(Focus::Library);
            }
        }
        if let Some(a) = self.sheets_area {
            if Self::rect_contains(&a, x, y) {
                return Some(Focus::Sheets);
            }
        }
        if let Some(a) = self.editor_area {
            if Self::rect_contains(&a, x, y) {
                return Some(Focus::Editor);
            }
        }
        None
    }

    /// Absolute paths of sheets currently open (single or multi-view).
    pub fn open_sheet_paths(&self) -> Vec<PathBuf> {
        if let Some(mv) = self.multiview.as_ref() {
            mv.segments.iter().map(|s| s.path.clone()).collect()
        } else if let Some(ed) = self.editor.as_ref() {
            vec![ed.path.clone()]
        } else {
            Vec::new()
        }
    }

    /// Group rels containing open sheets — the breadcrumb trail.
    pub fn open_groups(&self) -> HashSet<PathBuf> {
        self.open_sheet_paths()
            .iter()
            .filter_map(|p| {
                p.strip_prefix(&self.library_root)
                    .ok()
                    .and_then(|r| r.parent().map(|g| g.to_path_buf()))
            })
            .collect()
    }

    pub fn is_open_sheet(&self, abs: &Path) -> bool {
        if let Some(mv) = self.multiview.as_ref() {
            mv.segments.iter().any(|s| s.path == abs)
        } else {
            self.editor.as_ref().map(|e| e.path == abs).unwrap_or(false)
        }
    }

    /// Which separator (if any) owns this point. Only the right-edge
    /// border column counts, and only while panels are shown.
    pub fn separator_at(&self, x: u16, y: u16) -> Option<Separator> {
        if !self.show_panels || !self.mouse_enabled {
            return None;
        }
        let main = self.main_area?;
        if y < main.y || y >= main.y.saturating_add(main.height) {
            return None;
        }
        if let Some(a) = self.lib_area {
            if x == a.x.saturating_add(a.width.saturating_sub(1)) {
                return Some(Separator::Library);
            }
        }
        if let Some(a) = self.sheets_area {
            if x == a.x.saturating_add(a.width.saturating_sub(1)) {
                return Some(Separator::Sheets);
            }
        }
        None
    }

    /// Begin a separator drag. Captures live widths for clamping.
    pub fn start_drag(&mut self, sep: Separator) -> bool {
        let (Some(main), Some(lib), Some(sheets)) =
            (self.main_area, self.lib_area, self.sheets_area)
        else {
            return false;
        };
        if main.width == 0 {
            return false;
        }
        self.drag = Some(DragState {
            sep,
            main_x: main.x,
            main_w: main.width,
            lib_w: lib.width,
            sheets_w: sheets.width,
            orig_lib: self.lib_frac,
            orig_sheets: self.sheets_frac,
        });
        self.close_menu();
        self.status = "resizing panels… (Esc cancels)".to_string();
        true
    }

    /// Move the grabbed separator to terminal column `x`.
    pub fn drag_to(&mut self, x: u16) {
        let Some(d) = self.drag else {
            return;
        };
        if d.main_w == 0 {
            return;
        }
        match d.sep {
            Separator::Library => {
                let max = d
                    .main_w
                    .saturating_sub(d.sheets_w + EDITOR_MIN_W)
                    .max(MIN_PANE_W);
                let w = x.saturating_sub(d.main_x).saturating_add(1).clamp(MIN_PANE_W, max);
                self.lib_frac = w as f32 / d.main_w as f32;
            }
            Separator::Sheets => {
                let max = d
                    .main_w
                    .saturating_sub(d.lib_w + EDITOR_MIN_W)
                    .max(MIN_PANE_W);
                let w = x
                    .saturating_sub(d.main_x + d.lib_w)
                    .saturating_add(1)
                    .clamp(MIN_PANE_W, max);
                self.sheets_frac = w as f32 / d.main_w as f32;
            }
        }
    }

    /// Finish a drag: persist the split, or restore it when cancelled.
    pub fn end_drag(&mut self, cancel: bool) {
        if let Some(d) = self.drag.take() {
            if cancel {
                self.lib_frac = d.orig_lib;
                self.sheets_frac = d.orig_sheets;
                self.status = "resize cancelled".to_string();
            } else {
                self.persist_ui();
                self.status.clear();
            }
        }
    }

    /// Visible-group index for a Library row (title rows only).
    pub fn group_at_y(&self, y: u16) -> Option<usize> {
        self.lib_rows.iter().find(|(ry, _)| *ry == y).map(|(_, i)| *i)
    }

    /// Sheet-list index for a Sheets title row.
    pub fn sheet_at_y(&self, y: u16) -> Option<usize> {
        self.sheet_rows.iter().find(|(ry, _)| *ry == y).map(|(_, i)| *i)
    }

    /// Open-menu item index for a row inside the popup (border offset).
    pub fn menu_index_at(&self, y: u16) -> Option<usize> {
        let area = self.menu_area?;
        let idx = y.checked_sub(area.y + 1)? as usize;
        if idx < self.menu_items().len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Click mapping for multi-view: global row → (segment, char).
    /// Divider rows land on the start of the following segment.
    pub fn editor_point_to_mv(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        let inner = self.editor_inner?;
        let mv = self.multiview.as_ref()?;
        if x < inner.x || y < inner.y {
            return None;
        }
        let col = (x - inner.x) as usize;
        if !self.mv_map.is_empty() {
            let row = self.mv_map.get((y - inner.y) as usize)?;
            let s = mv.segments.get(row.seg)?;
            let off = crate::view::col_to_char(&row.text, col.min(inner.width as usize));
            let pos = s.rope.line_to_char(row.line) + row.char_start + off;
            return Some((row.seg, pos.min(s.rope.len_chars())));
        }
        // Pre-render fallback (same rope-line approximation as before,
        // made anchor-aware to match the new scroll semantics).
        let gline = mv
            .anchor
            .saturating_add(mv.scroll as usize)
            .saturating_add((y - inner.y) as usize);
        let (seg, line) = mv.locate(gline)?;
        let s = mv.segments.get(seg)?;
        Some((seg, s.clamp_to_line(line, col)))
    }

    /// Buffer char index for a click inside the editor text area.
    /// Uses the visual-row map recorded at render time; falls back to the
    /// old rope-line approximation before the first render.
    pub fn editor_point_to_char(&self, x: u16, y: u16) -> Option<usize> {
        let inner = self.editor_inner?;
        let ed = self.editor.as_ref()?;
        if x < inner.x || y < inner.y {
            return None;
        }
        let col = (x - inner.x) as usize;
        if !self.editor_map.is_empty() {
            let row = self.editor_map.get((y - inner.y) as usize)?;
            let off = crate::view::col_to_char(&row.text, col.min(inner.width as usize));
            return Some((ed.rope.line_to_char(row.line) + row.char_start + off).min(ed.rope.len_chars()));
        }
        let lines = ed.rope.len_lines().max(1);
        let line = self
            .editor_anchor
            .saturating_add(self.editor_scroll as usize)
            .saturating_add((y - inner.y) as usize)
            .min(lines - 1);
        let start = ed.rope.line_to_char(line);
        let mut len = ed.rope.line(line).len_chars();
        if len > 0 && ed.rope.get_char(start + len - 1) == Some('\n') {
            len -= 1;
        }
        Some(start + col.min(len))
    }

    fn rescan(&mut self) {
        let collapsed = std::mem::take(&mut self.library.collapsed);
        let mut lib = Library::scan(&self.library_root);
        lib.collapsed = collapsed;
        self.library = lib;
        // Fresh totals came from disk; reconcile open-buffer words on tick.
        self.stats_dirty = true;
    }

    pub fn create_sheet_tui(&mut self, title: &str) {
        self.move_lift = None;
        let group = self.selected_group().unwrap_or_default();
        let group_str = group.to_string_lossy().to_string();
        match Library::create_sheet(&self.library_root, &group_str, title) {
            Ok(abs) => {
                if let Some(name) = abs.file_name().and_then(|s| s.to_str()) {
                    self.library.order.append(&group_str, name);
                    self.library.save_order();
                }
                self.rescan();
                // point selection at the new sheet
                let g = self.selected_group();
                let sheets = self.library.sheets_for_group(g.as_deref());
                if let Some(i) = sheets.iter().position(|s| s.abs == abs) {
                    self.sheet_cursor = i;
                }
                self.open_sheet(&abs);
                self.focus = Focus::Editor;
                self.status = format!("created {}", abs.display());
            }
            Err(e) => self.status = format!("create failed: {e}"),
        }
    }

    /// Delete the open sheet (single mode) or the focused multi-view segment.
    pub fn delete_current_sheet(&mut self) {
        self.move_lift = None;
        if let Some(mv) = self.multiview.as_ref() {
            let (seg, _) = mv.cursor;
            let Some(path) = mv.segments.get(seg).map(|s| s.path.clone()) else {
                self.status = "nothing open to delete".to_string();
                return;
            };
            self.delete_sheet_paths(&[path]);
            return;
        }
        let path = match self.editor.as_ref() {
            Some(ed) => ed.path.clone(),
            None => {
                self.status = "nothing open to delete".to_string();
                return;
            }
        };
        self.delete_sheet_paths(&[path]);
    }

    /// Delete sheet files, fix order + selection, rescan, reopen fallback.
    fn delete_sheet_paths(&mut self, paths: &[PathBuf]) {
        let mut ok = 0;
        let mut missing = 0;
        for abs in paths {
            match std::fs::remove_file(abs) {
                Ok(()) => {
                    ok += 1;
                    if let Ok(rel) = abs.strip_prefix(&self.library_root) {
                        if let Some(parent) = rel.parent() {
                            if let Some(name) = rel.file_name().and_then(|s| s.to_str()) {
                                self.library
                                    .order
                                    .remove(&group_key(parent), name);
                            }
                        }
                    }
                    self.selected_sheets.remove(abs);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => missing += 1,
                Err(_) => missing += 1,
            }
        }
        self.library.save_order();
        self.pending_sheet_delete = None;
        // close single editor / drop stale multi-view segments
        if let Some(ed) = self.editor.as_ref() {
            if paths.contains(&ed.path) {
                self.editor = None;
                self.editor_words = 0;
            }
        }
        self.sheet_cursor = 0;
        self.rescan();
        self.clamp_cursors();
        self.sync_multiview();
        if self.multiview.is_none() {
            self.open_selected_sheet_fallback();
        }
        self.status = match (ok, missing) {
            (0, _) => "delete failed".to_string(),
            (1, 0) => "deleted".to_string(),
            (n, 0) => format!("deleted {n} sheets"),
            (n, m) => format!("deleted {n}, skipped {m}"),
        };
    }

    /// Start a rename prompt for the sheet under the cursor (Sheets pane).
    pub fn start_sheet_rename(&mut self) {
        if self.multiview.is_some() {
            self.status = "unmark sheets to rename (one at a time)".to_string();
            return;
        }
        let g = self.selected_group();
        let sheets = self.library.sheets_for_group(g.as_deref());
        let Some(s) = sheets.get(self.sheet_cursor.min(sheets.len().saturating_sub(1))) else {
            self.status = "no sheet to rename".to_string();
            return;
        };
        self.move_lift = None;
        self.sheet_rename_target = Some(s.abs.clone());
        self.new_prompt = Some(NewSheetPrompt { input: s.name.clone() });
    }

    /// Commit the sheet prompt: rename if a target is set, else create.
    pub fn commit_sheet_prompt(&mut self, title: &str) {
        self.close_menu();
        if let Some(target) = self.sheet_rename_target.take() {
            self.rename_sheet_tui(&target, title);
            return;
        }
        self.create_sheet_tui(title);
    }

    /// Open the in-buffer find prompt (Edit menu, or Ctrl+G from anywhere).
    /// Searches the open sheet — or the active multi-view segment.
    pub fn start_search(&mut self) {
        if self.move_lift.is_some() {
            self.status = "Esc cancels move first".to_string();
            return;
        }
        if self.editor.is_none() && self.multiview.is_none() {
            self.status = "nothing open to search".to_string();
            return;
        }
        self.close_menu();
        self.search_prompt = Some(SearchPrompt::default());
        self.search_last = None;
    }

    /// Match starts (char offsets) for `query` in the active buffer,
    /// case-insensitive, in document order.
    fn search_matches(&self, query: &str) -> Vec<usize> {
        let text = if let Some(mv) = self.multiview.as_ref() {
            let (seg, _) = mv.cursor;
            mv.segments.get(seg).map(|s| s.rope.to_string())
        } else {
            self.editor.as_ref().map(|ed| ed.rope.to_string())
        };
        text.map(|t| crate::words::find_matches_ci(&t, query))
            .unwrap_or_default()
    }

    /// Jump to the next match for the prompt's query (Enter in the prompt).
    /// Stays on the prompt so Enter keeps cycling; wraps past the last
    /// match. Esc closes the prompt.
    pub fn advance_search(&mut self) {
        let query = self
            .search_prompt
            .as_ref()
            .map(|p| p.input.trim().to_string())
            .unwrap_or_default();
        if query.is_empty() {
            self.search_prompt = None;
            self.search_last = None;
            self.status = "cancelled".to_string();
            return;
        }
        let matches = self.search_matches(&query);
        if matches.is_empty() {
            self.search_last = None;
            self.status = format!("no matches for \"{query}\"");
            return;
        }
        // Active cursor: the multi-view cursor wins when it is up.
        let cur = if let Some(mv) = self.multiview.as_ref() {
            mv.cursor.1
        } else if let Some(ed) = self.editor.as_ref() {
            ed.cursor
        } else {
            self.search_prompt = None;
            self.search_last = None;
            self.status = "nothing open to search".to_string();
            return;
        };
        // A repeated Enter right on the last jump continues strictly after
        // it; a moved cursor (or edited query) searches from the cursor.
        let from = match &self.search_last {
            Some((q, pos)) if q == &query && cur == *pos => pos + 1,
            _ => cur,
        };
        let (at, pos, wrapped) = match matches.iter().position(|&s| s >= from) {
            Some(i) => (i, matches[i], false),
            None => (0, matches[0], true),
        };
        if let Some(mv) = self.multiview.as_mut() {
            let (seg, _) = mv.cursor;
            if let Some(s) = mv.segments.get(seg) {
                mv.cursor = (seg, pos.min(s.rope.len_chars()));
            }
        } else if let Some(ed) = self.editor.as_mut() {
            ed.cursor = pos.min(ed.rope.len_chars());
        }
        self.search_last = Some((query.clone(), pos));
        self.status = if wrapped {
            format!("match 1 of {} (wrapped)", matches.len())
        } else {
            format!("match {} of {}", at + 1, matches.len())
        };
    }

    pub fn rename_sheet_tui(&mut self, old_abs: &Path, new_title: &str) {
        self.move_lift = None;
        let new_name = slugify(new_title);
        let new_abs = old_abs.with_file_name(&new_name);
        if new_abs == old_abs {
            self.status = "unchanged".to_string();
            return;
        }
        if new_abs.exists() {
            self.status = format!("already exists: {}", new_abs.display());
            return;
        }
        match std::fs::rename(old_abs, &new_abs) {
            Ok(()) => {
                if let Ok(rel) = old_abs.strip_prefix(&self.library_root) {
                    if let Some(parent) = rel.parent() {
                        let g = group_key(parent);
                        if let Some(old) = rel.file_name().and_then(|s| s.to_str()) {
                            self.library.order.rename(&g, old, &new_name);
                        }
                    }
                }
                self.library.save_order();
                if let Some(ed) = self.editor.as_mut() {
                    if ed.path == old_abs {
                        ed.path = new_abs.clone();
                    }
                }
                if let Some(mv) = self.multiview.as_mut() {
                    for s in mv.segments.iter_mut() {
                        if s.path == old_abs {
                            s.path = new_abs.clone();
                        }
                    }
                }
                if self.selected_sheets.remove(old_abs) {
                    self.selected_sheets.insert(new_abs.clone());
                }
                self.rescan();
                let g = self.selected_group();
                let sheets = self.library.sheets_for_group(g.as_deref());
                if let Some(i) = sheets.iter().position(|s| s.abs == new_abs) {
                    self.sheet_cursor = i;
                }
                self.clamp_cursors();
                self.open_sheet(&new_abs);
                self.status = format!("renamed to {new_name}");
            }
            Err(e) => self.status = format!("rename failed: {e}"),
        }
    }

    /// Two-step sheet delete (Sheets pane): victims = marked set, else cursor.
    /// Returns true if a delete was performed.
    pub fn delete_sheets_confirm(&mut self) -> bool {
        let g = self.selected_group();
        let sheets = self.library.sheets_for_group(g.as_deref());
        let current: Option<PathBuf> = sheets
            .get(self.sheet_cursor.min(sheets.len().saturating_sub(1)))
            .map(|s| s.abs.clone());
        let mut victims: Vec<PathBuf> = if !self.selected_sheets.is_empty() {
            let mut v: Vec<PathBuf> = self.selected_sheets.iter().cloned().collect();
            v.sort();
            v
        } else if let Some(c) = current {
            vec![c]
        } else {
            self.status = "no sheet to delete".to_string();
            return false;
        };
        victims.sort();
        victims.dedup();
        if self.pending_sheet_delete.as_ref() != Some(&victims) {
            self.pending_sheet_delete = Some(victims.clone());
            self.status = format!(
                "delete {} sheet(s)? press D again to confirm",
                victims.len()
            );
            return false;
        }
        self.move_lift = None;
        self.delete_sheet_paths(&victims);
        true
    }

    fn open_selected_sheet_fallback(&mut self) {
        let g = self.selected_group();
        let sheets = self.library.sheets_for_group(g.as_deref());
        if sheets.is_empty() {
            return;
        }
        let i = self.sheet_cursor.min(sheets.len() - 1);
        let abs = sheets[i].abs.clone();
        self.open_sheet(&abs);
    }

    fn clamp_cursors(&mut self) {
        let n = self.library.visible_groups().len();
        if n == 0 {
            self.lib_cursor = 0;
        } else {
            self.lib_cursor = self.lib_cursor.min(n - 1);
        }
        let g = self.selected_group();
        let m = self.library.sheets_for_group(g.as_deref()).len();
        if m == 0 {
            self.sheet_cursor = 0;
        } else {
            self.sheet_cursor = self.sheet_cursor.min(m - 1);
        }
    }

    /// Start a new-group prompt. Parents at the *same level* as the selection
    /// (parent of the selected group), so new groups don't nest unexpectedly;
    /// empty library = top level. Prompt shows the parent.
    pub fn start_new_group(&mut self) {
        self.move_lift = None;
        let parent = self
            .selected_group()
            .and_then(|g| g.parent().map(|p| p.to_path_buf()))
            .unwrap_or_default();
        self.group_prompt = Some(GroupPrompt {
            kind: GroupPromptKind::New { parent },
            input: String::new(),
        });
        self.pending_group_delete = None;
    }

    /// Start a rename prompt for the selected group.
    pub fn start_rename_group(&mut self) {
        self.move_lift = None;
        match self.selected_group() {
            Some(g) if !g.as_os_str().is_empty() => {
                let name = g
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                self.group_prompt = Some(GroupPrompt {
                    kind: GroupPromptKind::Rename { original: g },
                    input: name,
                });
                self.pending_group_delete = None;
            }
            _ => self.status = "no group to rename".to_string(),
        }
    }

    pub fn commit_group_prompt(&mut self) {
        let Some(p) = self.group_prompt.take() else {
            return;
        };
        match p.kind {
            GroupPromptKind::New { parent } => {
                match Library::create_group(&self.library_root, &parent, &p.input) {
                    Ok(_) => {
                        self.rescan();
                        // select the new group: find it in visible list
                        let target = if parent.as_os_str().is_empty() {
                            PathBuf::from(p.input.trim().replace(['/', '\\'], "-").trim())
                        } else {
                            parent.join(p.input.trim().replace(['/', '\\'], "-").trim())
                        };
                        let vis = self.library.visible_groups();
                        if let Some(i) = vis.iter().position(|g| g.rel == target) {
                            self.lib_cursor = i;
                        }
                        self.clamp_cursors();
                        self.sheet_cursor = 0;
                        self.focus = Focus::Library;
                        self.status = format!("created group {}", target.display());
                    }
                    Err(e) => self.status = format!("create group failed: {e}"),
                }
            }
            GroupPromptKind::Rename { original } => {
                match Library::rename_group(&self.library_root, &original, &p.input) {
                    Ok(new_rel) => {
                        let old_key = group_key(&original);
                        let new_key = group_key(&new_rel);
                        self.library.order.rename_group_prefix(&old_key, &new_key);
                        self.library.save_order();
                        self.remap_group_refs(&original, &new_rel);
                        // keep the open sheet if it lived inside the renamed group
                        if let Some(ed) = self.editor.as_mut() {
                            if let Ok(stripped) =
                                ed.path.strip_prefix(self.library_root.join(&original))
                            {
                                ed.path = self.library_root.join(&new_rel).join(stripped);
                            }
                        }
                        if let Some(mv) = self.multiview.as_mut() {
                            for s in mv.segments.iter_mut() {
                                if let Ok(stripped) =
                                    s.path.strip_prefix(self.library_root.join(&original))
                                {
                                    s.path = self.library_root.join(&new_rel).join(stripped);
                                }
                            }
                        }
                        self.rescan();
                        let vis = self.library.visible_groups();
                        if let Some(i) = vis.iter().position(|g| g.rel == new_rel) {
                            self.lib_cursor = i;
                        }
                        self.clamp_cursors();
                        self.status = format!("renamed to {}", new_rel.display());
                    }
                    Err(e) => self.status = format!("rename failed: {e}"),
                }
            }
        }
    }

    /// Remap selection + pending state when a group moves `old` → `new`.
    fn remap_group_refs(&mut self, old: &Path, new: &Path) {
        let map_one = |p: &PathBuf| -> PathBuf {
            if p == old {
                return new.to_path_buf();
            }
            if let Ok(rest) = p.strip_prefix(old) {
                return new.join(rest);
            }
            p.clone()
        };
        self.selected_groups = std::mem::take(&mut self.selected_groups)
            .into_iter()
            .map(|p| map_one(&p))
            .collect();
        if let Some(pending) = self.pending_group_delete.take() {
            self.pending_group_delete =
                Some(pending.into_iter().map(|p| map_one(&p)).collect());
        }
        // absolute sheet paths inside the moved group
        let old_abs = self.library_root.join(old);
        let new_abs = self.library_root.join(new);
        let map_abs = |p: &PathBuf| -> PathBuf {
            if let Ok(rest) = p.strip_prefix(&old_abs) {
                return new_abs.join(rest);
            }
            p.clone()
        };
        self.selected_sheets = std::mem::take(&mut self.selected_sheets)
            .into_iter()
            .map(|p| map_abs(&p))
            .collect();
        if let Some(pending) = self.pending_sheet_delete.take() {
            self.pending_sheet_delete =
                Some(pending.into_iter().map(|p| map_abs(&p)).collect());
        }
    }

    /// Two-step delete over marked groups (or the selected one).
    /// Returns true if a delete was performed.
    pub fn delete_group_confirm(&mut self) -> bool {
        let target = match self.selected_group() {
            Some(t) if !t.as_os_str().is_empty() => t,
            _ => {
                self.status = "no group to delete".to_string();
                return false;
            }
        };
        let mut victims: Vec<PathBuf> = if !self.selected_groups.is_empty()
            && self.selected_groups.contains(&target)
        {
            let mut v: Vec<PathBuf> = self.selected_groups.iter().cloned().collect();
            v.sort();
            v
        } else {
            vec![target]
        };
        victims.retain(|g| !g.as_os_str().is_empty());
        if victims.is_empty() {
            self.status = "no group to delete".to_string();
            return false;
        }
        if self.pending_group_delete.as_ref() != Some(&victims) {
            self.pending_group_delete = Some(victims.clone());
            let count = self
                .library
                .sheets
                .iter()
                .filter(|s| victims.iter().any(|t| s.group == *t || s.group.starts_with(t)))
                .count();
            self.status = if victims.len() == 1 {
                format!(
                    "delete {} ({} sheets)? press D again to confirm",
                    victims[0].display(),
                    count
                )
            } else {
                format!(
                    "delete {} groups ({} sheets)? press D again to confirm",
                    victims.len(),
                    count
                )
            };
            return false;
        }
        self.move_lift = None;
        self.pending_group_delete = None;
        let mut ok = 0;
        for t in &victims {
            if Library::delete_group(&self.library_root, t).is_ok() {
                ok += 1;
                self.library.order.remove_group_prefix(&group_key(t));
            }
        }
        self.library.save_order();
        for t in &victims {
            self.selected_groups.remove(t);
        }
        // close the editor if its sheet lived inside a deleted group
        let editor_gone = self.editor.as_ref().map(|ed| {
            victims
                .iter()
                .any(|t| ed.path.starts_with(self.library_root.join(t)))
        });
        if editor_gone == Some(true) {
            self.editor = None;
            self.editor_words = 0;
        }
        self.rescan();
        self.clamp_cursors();
        self.sync_multiview();
        if self.multiview.is_none() {
            self.open_selected_sheet_fallback();
        }
        self.status = if ok == victims.len() {
            if victims.len() == 1 {
                format!("deleted group {}", victims[0].display())
            } else {
                format!("deleted {} groups", victims.len())
            }
        } else {
            format!("deleted {ok} of {} groups", victims.len())
        };
        true
    }

    // --- marks ---

    pub fn toggle_mark_group(&mut self) {
        if self.move_lift.is_some() {
            self.status = "Esc cancels move first".to_string();
            return;
        }
        let Some(g) = self.selected_group() else {
            return;
        };
        if g.as_os_str().is_empty() {
            return;
        }
        if !self.selected_groups.remove(&g) {
            self.selected_groups.insert(g);
        }
        self.pending_group_delete = None;
    }

    pub fn toggle_mark_sheet(&mut self) {
        if self.move_lift.is_some() {
            self.status = "Esc cancels move first".to_string();
            return;
        }
        let g = self.selected_group();
        let sheets = self.library.sheets_for_group(g.as_deref());
        let Some(s) = sheets
            .get(self.sheet_cursor.min(sheets.len().saturating_sub(1)))
            .map(|s| s.abs.clone())
        else {
            return;
        };
        if !self.selected_sheets.remove(&s) {
            self.selected_sheets.insert(s);
        }
        self.pending_sheet_delete = None;
        self.sync_multiview();
    }

    /// Mark every sheet in the current group (Sheets `A`). A second press
    /// unmarks just this group's sheets, leaving other groups' marks alone.
    pub fn toggle_mark_all_sheets(&mut self) {
        if self.move_lift.is_some() {
            self.status = "Esc cancels move first".to_string();
            return;
        }
        let g = self.selected_group();
        let sheets = self.library.sheets_for_group(g.as_deref());
        if sheets.is_empty() {
            return;
        }
        if sheets.iter().all(|s| self.selected_sheets.contains(&s.abs)) {
            let mut n = 0;
            for s in &sheets {
                if self.selected_sheets.remove(&s.abs) {
                    n += 1;
                }
            }
            self.status = format!("unmarked {n} sheet{}", if n == 1 { "" } else { "s" });
        } else {
            let mut n = 0;
            for s in &sheets {
                if self.selected_sheets.insert(s.abs.clone()) {
                    n += 1;
                }
            }
            self.status = format!("marked {n} sheet{}", if n == 1 { "" } else { "s" });
        }
        self.pending_sheet_delete = None;
        self.sync_multiview();
    }

    // --- multi-view ---

    fn mv_sort_key(&self, abs: &Path) -> (String, usize, String) {
        match abs.strip_prefix(&self.library_root) {
            Ok(rel) => {
                let group = rel
                    .parent()
                    .map(group_key)
                    .unwrap_or_default();
                let name = rel
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_string();
                let rank = self.library.order.rank(&group, &name).unwrap_or(usize::MAX);
                (group, rank, name)
            }
            Err(_) => (
                String::new(),
                usize::MAX,
                abs.to_string_lossy().to_string(),
            ),
        }
    }

    fn mv_anchor(&self) -> Option<(PathBuf, usize, usize)> {
        let mv = self.multiview.as_ref()?;
        let (seg, c) = mv.cursor;
        let s = mv.segments.get(seg)?;
        let (line, col) = s.line_col(c);
        Some((s.path.clone(), line, col))
    }

    /// Rebuild (or clear) the sequential multi-sheet editor from marks.
    /// Flushes pending edits first so nothing is stranded.
    pub fn sync_multiview(&mut self) {
        // Drop missing files BEFORE flushing, so a delete never resurrects
        // a file by saving a stale dirty segment back to disk.
        self.selected_sheets.retain(|p| p.exists());
        if let Some(mv) = self.multiview.as_mut() {
            mv.segments.retain(|s| s.path.exists());
        }
        if self.ed_is_dirty() {
            self.save();
        }
        let mut paths: Vec<PathBuf> = self.selected_sheets.iter().cloned().collect();
        if paths.len() < 2 {
            self.multiview = None;
            return;
        }
        paths.sort_by_key(|a| self.mv_sort_key(a));
        let mut kept: Vec<PathBuf> = Vec::new();
        let mut bytes: u64 = 0;
        for p in &paths {
            let sz = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            if !kept.is_empty() && bytes + sz > MULTIVIEW_BYTES_CAP {
                break;
            }
            bytes += sz;
            kept.push(p.clone());
        }
        let truncated = kept.len() < paths.len();
        let anchor = self.mv_anchor();
        let mut segments = Vec::new();
        for p in &kept {
            if let Ok(text) = std::fs::read_to_string(p) {
                segments.push(Segment {
                    path: p.clone(),
                    rope: ropey::Rope::from_str(&text),
                    dirty: false,
                    history: History::default(),
                });
            }
        }
        if segments.len() < 2 {
            let have: HashSet<PathBuf> = segments.iter().map(|s| s.path.clone()).collect();
            self.selected_sheets.retain(|p| have.contains(p));
            self.multiview = None;
            return;
        }
        let mut mv = MultiView {
            segments,
            cursor: (0, 0),
            scroll: 0,
            anchor: 0,
        };
        if let Some((apath, aline, acol)) = anchor {
            if let Some(i) = mv.segments.iter().position(|s| s.path == apath) {
                let lines = mv.segments[i].rope.len_lines().max(1);
                let line = aline.min(lines - 1);
                mv.cursor = (i, mv.segments[i].clamp_to_line(line, acol));
                mv.scroll = mv.seg_start_line(i) as u16;
            }
        }
        self.editor_words = mv.words();
        self.multiview = Some(mv);
        // New segments: recenter on the restored cursor at next render.
        self.last_cursor_mv = None;
        if truncated {
            self.status = format!("multi-view truncated (size cap, {} shown)", kept.len());
        }
    }

    // --- move mode ---

    /// Lift marked items (or the current one) for a move.
    pub fn start_move(&mut self) {
        let mut lift = MoveLift::default();
        match self.focus {
            Focus::Library => {
                if !self.selected_groups.is_empty() {
                    let mut v: Vec<PathBuf> = self.selected_groups.iter().cloned().collect();
                    v.sort();
                    lift.groups = v;
                } else if let Some(g) = self.selected_group() {
                    if !g.as_os_str().is_empty() {
                        lift.groups = vec![g];
                    }
                }
            }
            Focus::Sheets => {
                if !self.selected_sheets.is_empty() {
                    let mut v: Vec<PathBuf> = self.selected_sheets.iter().cloned().collect();
                    v.sort();
                    lift.sheets = v;
                } else {
                    let g = self.selected_group();
                    let sheets = self.library.sheets_for_group(g.as_deref());
                    if let Some(s) = sheets
                        .get(self.sheet_cursor.min(sheets.len().saturating_sub(1)))
                    {
                        lift.sheets = vec![s.abs.clone()];
                    }
                }
            }
            Focus::Editor => {
                if let Some(mv) = self.multiview.as_ref() {
                    if let Some(s) = mv.segments.get(mv.cursor.0) {
                        lift.sheets = vec![s.path.clone()];
                    }
                } else if let Some(ed) = self.editor.as_ref() {
                    lift.sheets = vec![ed.path.clone()];
                }
            }
        }
        if lift.is_empty() {
            self.status = "nothing to move".to_string();
            return;
        }
        self.close_menu();
        self.move_lift = Some(lift);
        self.status = "move mode: navigate, Enter drops here, Esc cancels".to_string();
    }

    /// Dynamic move banner for the status bar (target follows navigation).
    pub fn move_banner(&self) -> Option<String> {
        let lift = self.move_lift.as_ref()?;
        let target = self
            .selected_group()
            .map(|g| g.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "…".to_string());
        Some(format!(
            "MOVE ({}) → {}  |  ",
            lift.count(),
            target
        ))
    }

    /// Swap the cursor sheet with its neighbor in manual order.
    pub fn swap_sheet(&mut self, dir: i32) {
        let g = self.selected_group().unwrap_or_default();
        let gstr = group_key(&g);
        let sheets = self.library.sheets_for_group(Some(&g));
        let names: Vec<String> = sheets.iter().map(|s| s.name.clone()).collect();
        let Some(cur) = sheets.get(self.sheet_cursor.min(names.len().saturating_sub(1))) else {
            return;
        };
        let cur_name = cur.name.clone();
        match self.library.order.swap_neighbor(&gstr, &names, &cur_name, dir) {
            Some(to) => {
                self.library.save_order();
                self.rescan();
                self.sheet_cursor = to.min(
                    self.library.sheets_for_group(Some(&g)).len().saturating_sub(1),
                );
                self.status = format!("moved {cur_name}");
            }
            None => self.status = "can't reorder".to_string(),
        }
    }

    /// Drop lifted items into the selected group.
    pub fn drop_move(&mut self) {
        let Some(lift) = self.move_lift.take() else {
            return;
        };
        // Flush first: renames below move files out from under open buffers.
        if self.ed_is_dirty() {
            self.save();
        }
        let Some(target) = self.selected_group() else {
            self.move_lift = Some(lift);
            self.status = "no target group".to_string();
            return;
        };
        if target.as_os_str().is_empty() {
            self.move_lift = Some(lift);
            self.status = "no target group".to_string();
            return;
        }
        let tstr = group_key(&target);
        let mut moved = 0;
        let mut skipped = 0;
        // (old_abs, new_abs) pairs to remap open buffers
        let mut remaps: Vec<(PathBuf, PathBuf)> = Vec::new();

        // Drop-before position when focused on a sheet of the target group.
        let before: Option<String> = if self.focus == Focus::Sheets {
            let sheets = self.library.sheets_for_group(Some(&target));
            sheets
                .get(self.sheet_cursor.min(sheets.len().saturating_sub(1)))
                .filter(|s| s.group == target)
                .map(|s| s.name.clone())
        } else {
            None
        };

        // --- sheets ---
        if !lift.sheets.is_empty() {
            // names currently in the target group
            let existing: Vec<String> = self
                .library
                .sheets
                .iter()
                .filter(|s| s.group == target)
                .map(|s| s.name.clone())
                .collect();
            let mut newcomers: Vec<(PathBuf, String)> = Vec::new(); // (old abs, name)
            for abs in &lift.sheets {
                let Some(name) = abs.file_name().and_then(|s| s.to_str()).map(|s| s.to_string())
                else {
                    skipped += 1;
                    continue;
                };
                let parent = abs
                    .strip_prefix(&self.library_root)
                    .ok()
                    .and_then(|r| r.parent().map(|p| p.to_path_buf()));
                if parent.as_deref() == Some(&target) {
                    continue; // stayer: handled by reorder below
                }
                newcomers.push((abs.clone(), name));
            }
            let mut actually_moved: Vec<String> = Vec::new();
            for (abs, name) in &newcomers {
                let dest = self.library_root.join(&target).join(name);
                if dest == *abs {
                    continue;
                }
                if dest.exists() {
                    skipped += 1;
                    continue;
                }
                match std::fs::rename(abs, &dest) {
                    Ok(()) => {
                        if let Ok(rel) = abs.strip_prefix(&self.library_root) {
                            if let Some(parent) = rel.parent() {
                                self.library.order.remove(&group_key(parent), name);
                            }
                        }
                        remaps.push((abs.clone(), dest));
                        actually_moved.push(name.clone());
                        moved += 1;
                    }
                    Err(_) => skipped += 1,
                }
            }
            // stayers already in target: reorder the lifted ones among them
            let stayers: Vec<String> = lift
                .sheets
                .iter()
                .filter_map(|abs| {
                    let name = abs.file_name()?.to_str()?.to_string();
                    let parent = abs
                        .strip_prefix(&self.library_root)
                        .ok()?
                        .parent()?
                        .to_path_buf();
                    (parent == target).then_some(name)
                })
                .collect();
            if !stayers.is_empty() && before.is_some() {
                // pure reorder when nothing moved across groups
                let mut disp = self
                    .library
                    .order
                    .display_order(&tstr, &self.target_names(&target));
                disp.retain(|n| !stayers.contains(n));
                let pos = before
                    .as_ref()
                    .filter(|b| !stayers.contains(*b))
                    .and_then(|b| disp.iter().position(|n| n == b))
                    .unwrap_or(disp.len());
                for (k, n) in stayers.iter().enumerate() {
                    disp.insert(pos + k, n.clone());
                }
                self.library.order.set_order(&tstr, disp);
                moved += stayers.len();
            } else if !actually_moved.is_empty() {
                let mut combined = existing;
                for n in &actually_moved {
                    if !combined.contains(n) {
                        combined.push(n.clone());
                    }
                }
                let mut disp = self.library.order.display_order(&tstr, &combined);
                disp.retain(|n| !actually_moved.contains(n));
                let pos = before
                    .as_ref()
                    .filter(|b| !actually_moved.contains(b))
                    .and_then(|b| disp.iter().position(|n| n == b))
                    .unwrap_or(disp.len());
                for (k, n) in actually_moved.iter().enumerate() {
                    disp.insert(pos + k, n.clone());
                }
                self.library.order.set_order(&tstr, disp);
            }
            let _ = existing;
        }

        // --- groups ---
        for g in &lift.groups {
            if target.starts_with(g) {
                skipped += 1; // into itself or a descendant
                continue;
            }
            let Some(name) = g.file_name().and_then(|s| s.to_str()) else {
                skipped += 1;
                continue;
            };
            let dest = self.library_root.join(&target).join(name);
            if dest == self.library_root.join(g) {
                continue; // no-op
            }
            if dest.exists() {
                skipped += 1;
                continue;
            }
            match std::fs::rename(self.library_root.join(g), &dest) {
                Ok(()) => {
                    let new_rel = target.join(name);
                    self.library
                        .order
                        .rename_group_prefix(&group_key(g), &group_key(&new_rel));
                    self.remap_group_refs(g, &new_rel);
                    remaps.push((self.library_root.join(g), dest));
                    moved += 1;
                }
                Err(_) => skipped += 1,
            }
        }

        // open buffers follow their files
        for (old, new) in &remaps {
            if let Some(ed) = self.editor.as_mut() {
                if ed.path == *old {
                    ed.path = new.clone();
                } else if let Ok(rest) = ed.path.strip_prefix(old) {
                    ed.path = new.join(rest);
                }
            }
            if let Some(mv) = self.multiview.as_mut() {
                for s in mv.segments.iter_mut() {
                    if s.path == *old {
                        s.path = new.clone();
                    } else if let Ok(rest) = s.path.strip_prefix(old) {
                        s.path = new.join(rest);
                    }
                }
            }
        }

        self.library.save_order();
        self.selected_groups.clear();
        self.selected_sheets.clear();
        self.pending_group_delete = None;
        self.pending_sheet_delete = None;
        self.rescan();
        self.clamp_cursors();
        // select the target group, cursor on first result
        let vis = self.library.visible_groups();
        if let Some(i) = vis.iter().position(|x| x.rel == target) {
            self.lib_cursor = i;
        }
        self.clamp_cursors();
        self.sync_multiview();
        if self.focus != Focus::Editor {
            self.focus = Focus::Sheets;
        }
        self.status = if skipped == 0 {
            format!("moved {moved}")
        } else {
            format!("moved {moved}, skipped {skipped}")
        };
    }

    /// Current sheet names in a group (for order computations).
    fn target_names(&self, target: &Path) -> Vec<String> {
        self.library
            .sheets
            .iter()
            .filter(|s| s.group == target)
            .map(|s| s.name.clone())
            .collect()
    }

    pub fn clear_group_pending(&mut self) {
        if self.pending_group_delete.take().is_some() {
            self.status.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::OrderFile;

    fn tmp_app(tag: &str) -> (App, PathBuf) {
        let dir = std::env::temp_dir().join(format!("alcott-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = App::new(dir.join("Library"), dir.join("data"));
        (app, dir)
    }

    fn seed_library(lib: &Path) {
        Library::create_group(lib, Path::new(""), "G1").unwrap();
        Library::create_group(lib, Path::new(""), "G2").unwrap();
        Library::create_sheet(lib, "G1", "B").unwrap();
        Library::create_sheet(lib, "G1", "A").unwrap();
        Library::create_sheet(lib, "G1", "C").unwrap();
        Library::create_sheet(lib, "G2", "S2").unwrap();
    }

    fn sheet_names(app: &App, group: &str) -> Vec<String> {
        app.library
            .sheets_for_group(Some(&PathBuf::from(group)))
            .iter()
            .map(|s| s.name.clone())
            .collect()
    }

    #[test]
    fn manual_order_scan_and_swap() {
        let dir = std::env::temp_dir().join(format!("alcott-ord-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("Library");
        seed_library(&lib);
        let mut app = App::new(lib.clone(), dir.join("data"));
        // alphabetical baseline
        assert_eq!(sheet_names(&app, "G1"), vec!["A.md", "B.md", "C.md"]);
        // swap B (cursor 1) down: A C B
        app.sheet_cursor = 1;
        app.swap_sheet(1);
        assert_eq!(sheet_names(&app, "G1"), vec!["A.md", "C.md", "B.md"]);
        assert_eq!(app.sheet_cursor, 2);
        // persisted across reload
        let app2 = App::new(lib.clone(), dir.join("data"));
        let names2: Vec<String> = app2
            .library
            .sheets_for_group(Some(&PathBuf::from("G1")))
            .iter()
            .map(|s| s.name.clone())
            .collect();
        assert_eq!(names2, vec!["A.md", "C.md", "B.md"]);
        // order file materialized the swap
        let order = OrderFile::load(&lib);
        assert_eq!(order.order.get("G1").unwrap(), &vec!["A.md", "C.md", "B.md"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn new_group_parents_at_sibling_level() {
        let dir = std::env::temp_dir().join(format!("alcott-sib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("Library");
        Library::create_group(&lib, Path::new(""), "Writing").unwrap();
        Library::create_group(&lib, &PathBuf::from("Writing"), "Book 1").unwrap();
        let mut app = App::new(lib.clone(), dir.join("data"));
        // select Writing/Book 1 (visible index 1)
        app.lib_cursor = 1;
        app.start_new_group();
        let parent = match app.group_prompt.as_ref().unwrap().kind.clone() {
            GroupPromptKind::New { parent } => parent,
            _ => panic!("expected New"),
        };
        assert_eq!(parent, PathBuf::from("Writing"));
        app.group_prompt.as_mut().unwrap().input = "Book 2".into();
        app.commit_group_prompt();
        assert!(lib.join("Writing").join("Book 2").is_dir());
        assert!(!lib.join("Writing").join("Book 1").join("Book 2").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_sheet_updates_order_and_selection() {
        let dir = std::env::temp_dir().join(format!("alcott-ren-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("Library");
        seed_library(&lib);
        let mut app = App::new(lib.clone(), dir.join("data"));
        app.sheet_cursor = 0; // A.md in G1 (lib_cursor 0 = G1)
        let old = lib.join("G1").join("A.md");
        app.rename_sheet_tui(&old, "Alpha");
        assert!(lib.join("G1").join("Alpha.md").exists());
        assert!(!old.exists());
        let names = sheet_names(&app, "G1");
        assert!(names.contains(&"Alpha.md".to_string()));
        assert!(!names.contains(&"A.md".to_string()));
        let order = OrderFile::load(&lib);
        assert!(order.order.get("G1").unwrap().contains(&"Alpha.md".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_sheet_between_groups() {
        let dir = std::env::temp_dir().join(format!("alcott-mv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("Library");
        seed_library(&lib);
        let mut app = App::new(lib.clone(), dir.join("data"));
        // mark A.md in G1 (lib_cursor 0, sheet 0)
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        app.focus = Focus::Sheets;
        app.start_move();
        assert!(app.move_lift.is_some());
        // navigate to G2 (visible index 1) and drop
        app.lib_cursor = 1;
        app.drop_move();
        assert!(lib.join("G2").join("A.md").exists());
        assert!(!lib.join("G1").join("A.md").exists());
        assert!(sheet_names(&app, "G2").contains(&"A.md".to_string()));
        assert!(app.selected_sheets.is_empty());
        let order = OrderFile::load(&lib);
        assert!(!order.order.get("G1").unwrap_or(&vec![]).contains(&"A.md".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_group_into_descendant_is_refused() {
        let dir = std::env::temp_dir().join(format!("alcott-mvg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("Library");
        Library::create_group(&lib, Path::new(""), "G").unwrap();
        Library::create_group(&lib, &PathBuf::from("G"), "Sub").unwrap();
        let mut app = App::new(lib.clone(), dir.join("data"));
        app.lib_cursor = 0; // G
        app.toggle_mark_group();
        app.focus = Focus::Library;
        app.start_move();
        app.lib_cursor = 1; // G/Sub
        app.drop_move();
        // refused: still in place
        assert!(lib.join("G").join("Sub").is_dir());
        assert!(!lib.join("G").join("Sub").join("G").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiview_edits_stay_in_their_files() {
        let dir = std::env::temp_dir().join(format!("alcott-mvx-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("Library");
        Library::create_group(&lib, Path::new(""), "G").unwrap();
        Library::create_sheet(&lib, "G", "One").unwrap();
        Library::create_sheet(&lib, "G", "Two").unwrap();
        let mut app = App::new(lib.clone(), dir.join("data"));
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        app.sheet_cursor = 1;
        app.toggle_mark_sheet();
        let mv = app.multiview.as_ref().expect("multi-view active");
        assert_eq!(mv.segments.len(), 2);
        assert_eq!(app.editor_words, mv.words());
        // type in first segment only
        app.focus = Focus::Editor;
        app.ed_insert_char('x');
        {
            let mv = app.multiview.as_ref().unwrap();
            assert!(mv.segments[0].dirty);
            assert!(!mv.segments[1].dirty);
        }
        // move to second segment, type there
        {
            let mv = app.multiview.as_mut().unwrap();
            mv.cursor = (1, 0);
        }
        app.ed_insert_char('y');
        app.save();
        let one = std::fs::read_to_string(lib.join("G").join("One.md")).unwrap();
        let two = std::fs::read_to_string(lib.join("G").join("Two.md")).unwrap();
        assert!(one.starts_with('x'));
        assert!(two.starts_with('y'));
        assert!(!one.contains('y') && !two.contains('x'));
        // no divider leaked into either file
        assert!(!one.contains('─') && !two.contains('─'));
        // divider rows map to the following segment
        let inner = Rect::new(10, 5, 60, 20);
        app.editor_inner = Some(inner);
        app.editor_scroll = 0;
        let l0 = app.multiview.as_ref().unwrap().segments[0].rope.len_lines();
        // divider row is at global line l0
        let mapped = app.editor_point_to_mv(10, 5 + l0 as u16);
        assert_eq!(mapped.map(|m| m.0), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_sheet_respects_marks_and_order() {
        let dir = std::env::temp_dir().join(format!("alcott-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib = dir.join("Library");
        seed_library(&lib);
        let mut app = App::new(lib.clone(), dir.join("data"));
        // mark A.md and B.md
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        app.sheet_cursor = 1;
        app.toggle_mark_sheet();
        // first D arms, second confirms
        assert!(!app.delete_sheets_confirm());
        assert!(app.delete_sheets_confirm());
        assert!(!lib.join("G1").join("A.md").exists());
        assert!(!lib.join("G1").join("B.md").exists());
        assert!(lib.join("G1").join("C.md").exists());
        let order = OrderFile::load(&lib);
        let g1 = order.order.get("G1").cloned().unwrap_or_default();
        assert!(!g1.contains(&"A.md".to_string()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mouse_on_by_default_and_persists() {
        let (mut app, dir) = tmp_app("mouse-def");
        assert!(app.mouse_enabled);
        assert!(!app.toggle_mouse());
        let reloaded = UiState::load(&dir.join("data"));
        assert!(!reloaded.mouse_enabled);
        assert!(reloaded.show_panels);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn separator_hit_testing() {
        use crate::app::Separator;
        let (mut app, dir) = tmp_app("sep-hit");
        app.show_panels = true;
        app.mouse_enabled = true;
        app.main_area = Some(Rect::new(0, 1, 120, 20));
        app.lib_area = Some(Rect::new(0, 1, 30, 20));
        app.sheets_area = Some(Rect::new(30, 1, 30, 20));
        // right-edge border columns
        assert_eq!(app.separator_at(29, 5), Some(Separator::Library));
        assert_eq!(app.separator_at(59, 5), Some(Separator::Sheets));
        // content columns are not separators
        assert_eq!(app.separator_at(28, 5), None);
        assert_eq!(app.separator_at(10, 5), None);
        // outside the main area vertically
        assert_eq!(app.separator_at(29, 0), None);
        assert_eq!(app.separator_at(29, 21), None);
        // hidden panels or disabled mouse: no hit
        app.show_panels = false;
        assert_eq!(app.separator_at(29, 5), None);
        app.show_panels = true;
        app.mouse_enabled = false;
        assert_eq!(app.separator_at(29, 5), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drag_clamps_to_minimums() {
        use crate::app::Separator;
        let (mut app, dir) = tmp_app("sep-drag");
        app.main_area = Some(Rect::new(0, 1, 120, 20));
        app.lib_area = Some(Rect::new(0, 1, 30, 20));
        app.sheets_area = Some(Rect::new(30, 1, 30, 20));
        // library separator: drag far left clamps to 12 cols
        assert!(app.start_drag(Separator::Library));
        app.drag_to(0);
        assert!((app.lib_frac * 120.0).round() >= 12.0);
        // drag far right keeps 30 cols for the editor
        app.drag_to(200);
        let w = (app.lib_frac * 120.0).round() as u16;
        assert!(w + 30 + 30 <= 120, "editor keeps 30 cols, got lib {w}");
        app.end_drag(false);
        assert!(app.drag.is_none());
        // sheets separator respects its own minimum
        assert!(app.start_drag(Separator::Sheets));
        let before = (app.lib_frac, app.sheets_frac);
        app.drag_to(0);
        assert!((app.sheets_frac * 120.0).round() >= 12.0);
        app.drag_to(100);
        assert!((app.sheets_frac * 120.0).round() > 12.0);
        // Esc restores the pre-drag split (without persisting)
        app.end_drag(true);
        assert_eq!((app.lib_frac, app.sheets_frac), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_persists_across_restarts() {
        use crate::app::Separator;
        let (mut app, dir) = tmp_app("sep-persist");
        app.main_area = Some(Rect::new(0, 1, 120, 20));
        app.lib_area = Some(Rect::new(0, 1, 30, 20));
        app.sheets_area = Some(Rect::new(30, 1, 30, 20));
        assert!(app.start_drag(Separator::Library));
        app.drag_to(48); // ~40% of 120
        app.end_drag(false); // persists + clears
        assert!(app.drag.is_none());
        let reloaded = UiState::load(&dir.join("data"));
        assert!((reloaded.lib_frac - app.lib_frac).abs() < f32::EPSILON);
        assert!((app.lib_frac * 120.0).round() >= 40.0);
        // old state files without the keys still load (serde defaults)
        std::fs::write(
            dir.join("data").join("state.json"),
            r#"{"show_panels":true,"mouse_enabled":true}"#,
        )
        .unwrap();
        let legacy = UiState::load(&dir.join("data"));
        assert!((legacy.lib_frac - 0.25).abs() < f32::EPSILON);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mouse_hit_testing() {
        let (mut app, dir) = tmp_app("mouse-hit");
        app.lib_area = Some(Rect::new(0, 1, 20, 8));
        app.sheets_area = Some(Rect::new(20, 1, 20, 8));
        app.editor_area = Some(Rect::new(40, 1, 60, 8));
        assert_eq!(app.pane_at(2, 3), Some(Focus::Library));
        assert_eq!(app.pane_at(25, 3), Some(Focus::Sheets));
        assert_eq!(app.pane_at(50, 3), Some(Focus::Editor));
        assert_eq!(app.pane_at(200, 200), None);

        app.lib_rows = vec![(2, 0), (3, 1)];
        assert_eq!(app.group_at_y(3), Some(1));
        assert_eq!(app.group_at_y(9), None);
        app.sheet_rows = vec![(4, 2)];
        assert_eq!(app.sheet_at_y(4), Some(2));

        // menu rows: border offset of 1
        app.open_menu = Some(MenuKind::View);
        app.menu_area = Some(Rect::new(12, 1, 30, 9));
        assert_eq!(app.menu_index_at(1), None);
        assert_eq!(app.menu_index_at(2), Some(0));
        assert_eq!(app.menu_index_at(2 + app.menu_items().len() as u16), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_click_mapping() {
        let (mut app, dir) = tmp_app("mouse-ed");
        let abs =
            Library::create_sheet(&app.library_root, "", "Click").expect("create sheet");
        app.open_sheet(&abs);
        // text is "# Click\n\n": line 0 is 7 wide, line 1 empty
        app.editor_inner = Some(Rect::new(10, 5, 40, 10));
        app.editor_scroll = 0;
        assert_eq!(app.editor_point_to_char(10, 5), Some(0));
        assert_eq!(app.editor_point_to_char(12, 5), Some(2));
        // past EOL clamps to line end
        assert_eq!(app.editor_point_to_char(50, 5), Some(7));
        // second line (empty)
        let l1 = app.editor.as_ref().unwrap().rope.line_to_char(1);
        assert_eq!(app.editor_point_to_char(10, 6), Some(l1));
        // outside the text box
        assert_eq!(app.editor_point_to_char(9, 5), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zathura_tmp_paths_are_stable_per_format() {
        use crate::export::ExportFormat;
        let pdf = App::zathura_tmp_path(ExportFormat::Pdf);
        let epub = App::zathura_tmp_path(ExportFormat::Epub);
        assert!(pdf.extension().and_then(|s| s.to_str()) == Some("pdf"));
        assert!(epub.extension().and_then(|s| s.to_str()) == Some("epub"));
        assert_ne!(pdf, epub);
        assert_eq!(pdf, App::zathura_tmp_path(ExportFormat::Pdf));
    }

    #[test]
    fn zathura_preview_rejects_other_formats() {
        let (mut app, dir) = tmp_app("zath-noop");
        for fmt in [
            crate::export::ExportFormat::Md,
            crate::export::ExportFormat::Html,
            crate::export::ExportFormat::Rtf,
            crate::export::ExportFormat::Docx,
        ] {
            app.export_format = fmt;
            app.export_zathura_path = None;
            assert!(!app.preview_pdf_epub_zathura());
            assert!(app.export_zathura_path.is_none());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn saving_refreshes_active_zathura_preview() {
        use crate::export::ExportFormat;
        let (mut app, dir) = tmp_app("zath-save-refresh");
        let abs = Library::create_sheet(&app.library_root, "", "S").unwrap();
        std::fs::write(&abs, "# S\n\nfirst version\n").unwrap();
        app.library = Library::scan(&app.library_root);
        app.open_sheet(&abs);
        // Simulate a live zathura preview with the export dialog closed.
        // Refreshes always target the stable per-format tmp file.
        app.export_format = ExportFormat::Epub;
        app.show_export = false;
        let preview = App::zathura_tmp_path(ExportFormat::Epub);
        let _ = std::fs::remove_file(&preview);
        app.export_zathura_path = Some(preview.clone());
        app.export_zathura_child = None;
        // Baseline render, then edit + save.
        app.refresh_zathura_preview();
        let before = std::fs::read(&preview).expect("preview written");
        assert!(!before.is_empty());
        if let Some(ed) = app.editor.as_mut() {
            ed.cursor = ed.rope.len_chars();
            ed.insert_char('!');
        }
        app.on_edit_no_word();
        app.save();
        assert_eq!(app.status, "saved");
        let after = std::fs::read(&preview).expect("preview still there");
        assert_ne!(before, after, "save rewrote the live preview");
        let _ = std::fs::remove_file(&preview);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dead_zathura_viewer_state_cleared_on_save() {
        use crate::export::ExportFormat;
        let (mut app, dir) = tmp_app("zath-dead");
        let abs = Library::create_sheet(&app.library_root, "", "S").unwrap();
        std::fs::write(&abs, "# S\n\nbody\n").unwrap();
        app.library = Library::scan(&app.library_root);
        app.open_sheet(&abs);
        app.export_format = ExportFormat::Epub;
        let preview = dir.join("preview.epub");
        std::fs::write(&preview, b"stale-bytes").unwrap();
        // Viewer exited meanwhile: stale state must be dropped, file kept.
        let mut child = std::process::Command::new("true")
            .spawn()
            .expect("spawn true");
        child.wait().expect("reap");
        app.export_zathura_path = Some(preview.clone());
        app.export_zathura_child = Some(child);
        if let Some(ed) = app.editor.as_mut() {
            ed.cursor = ed.rope.len_chars();
            ed.insert_char('!');
        }
        app.on_edit_no_word();
        app.save();
        assert_eq!(app.status, "saved");
        assert!(app.export_zathura_path.is_none());
        assert!(app.export_zathura_child.is_none());
        assert_eq!(std::fs::read(&preview).unwrap(), b"stale-bytes");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zathura_refresh_is_noop_without_active_viewer() {
        let (mut app, dir) = tmp_app("zath-refresh");
        app.show_export = true;
        app.export_zathura_path = None;
        // Must not create files or panic when no viewer is active.
        app.refresh_zathura_preview();
        assert!(app.export_zathura_path.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stats_debounce_and_reconcile() {
        let (mut app, dir) = tmp_app("stats-deb");
        let abs = Library::create_sheet(&app.library_root, "", "S").unwrap();
        std::fs::write(&abs, "# S\n\nhello world\n").unwrap();
        app.library = Library::scan(&app.library_root);
        app.open_sheet(&abs);
        let total_before = app.library.total_words;
        assert!(!app.stats_dirty);
        // Typing marks stats dirty but does not recount immediately.
        app.focus = Focus::Editor;
        app.ed_insert_char('x');
        assert!(app.stats_dirty);
        assert_eq!(app.library.total_words, total_before);
        // First tick refresh reconciles; an immediate second tick is throttled.
        assert!(app.maybe_refresh_stats(false));
        assert!(!app.stats_dirty);
        assert!(app.library.total_words >= total_before);
        app.ed_insert_char('y');
        assert!(app.stats_dirty);
        assert!(!app.maybe_refresh_stats(false));
        assert!(app.stats_dirty);
        // Forced refresh (e.g. on save/switch) always runs.
        assert!(app.maybe_refresh_stats(true));
        assert!(!app.stats_dirty);
        // Saving also reconciles.
        app.ed_insert_char('z');
        assert!(app.stats_dirty);
        app.save();
        assert!(!app.stats_dirty);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multiview_visual_up_down_hops_segments() {
        // Seg 0 wraps to two visual rows at width 10 (chars 0-9, then
        // 11-20); seg 1 is a short second sheet.
        let seg0 = Segment {
            path: PathBuf::from("/tmp/a.md"),
            rope: ropey::Rope::from_str("abcdefghij klmnopqrst"),
            dirty: false,
            history: History::default(),
        };
        let seg1 = Segment {
            path: PathBuf::from("/tmp/b.md"),
            rope: ropey::Rope::from_str("hi"),
            dirty: false,
            history: History::default(),
        };
        let mut mv = MultiView {
            segments: vec![seg0, seg1],
            cursor: (0, 15),
            scroll: 0,
            anchor: 0,
        };
        // Mid second row -> same visual column in the first row (Up mirrors
        // Down instead of snapping to the row end).
        mv.move_up_visual(10);
        assert_eq!(mv.cursor, (0, 4));
        // Back down restores the column: Up/Down are inverses here.
        mv.move_down_visual(10);
        assert_eq!(mv.cursor, (0, 15));
        // Up from a wrapped row's start lands on the previous row's start.
        mv.cursor = (0, 11);
        mv.move_up_visual(10);
        assert_eq!(mv.cursor, (0, 0));
        // Last visual row -> hops into the next segment (clamped).
        mv.cursor = (0, 15);
        mv.move_down_visual(10);
        mv.move_down_visual(10);
        assert_eq!(mv.cursor, (1, 2));
        // Back up preserves the column into seg 0.
        mv.move_up_visual(10);
        assert_eq!(mv.cursor, (0, 2));
        // Width unknown falls back to rope lines.
        mv.cursor = (1, 1);
        mv.move_up_visual(0);
        assert_eq!(mv.cursor, (0, 1));
    }


    #[test]
    fn mark_all_sheets_toggles_group_scope() {
        let (mut app, dir) = tmp_app("mark-all");
        let lib = dir.join("Library");
        seed_library(&lib);
        Library::create_group(&lib, std::path::Path::new(""), "Empty").unwrap();
        app.library = Library::scan(&lib);
        // Mark one sheet in another group first.
        app.lib_cursor = 2; // G2
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        assert_eq!(app.selected_sheets.len(), 1);
        // Empty group: no-op, nothing panics.
        app.lib_cursor = 0; // Empty
        app.toggle_mark_all_sheets();
        assert_eq!(app.selected_sheets.len(), 1);
        // Mark all in G1 (A, B, C); the G2 mark survives.
        app.lib_cursor = 1;
        app.sheet_cursor = 0;
        app.toggle_mark_all_sheets();
        assert_eq!(app.selected_sheets.len(), 4);
        assert!(app.multiview.is_some(), "2+ marks drive multi-view");
        assert!(app.status.contains("marked 3"), "status: {}", app.status);
        // Second press unmarks only G1's sheets.
        app.toggle_mark_all_sheets();
        assert_eq!(app.selected_sheets.len(), 1);
        assert!(app.multiview.is_none());
        assert!(app.status.contains("unmarked"), "status: {}", app.status);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zathura_liveness_tracks_child_exit() {
        // No handle means no live viewer.
        assert!(!App::zathura_alive(&mut None));
        // A reaped child reports dead so the next preview relaunches.
        let mut child = Some(
            std::process::Command::new("true")
                .spawn()
                .expect("spawn true"),
        );
        let status = child.as_mut().unwrap().wait().expect("reap");
        assert!(status.success());
        assert!(!App::zathura_alive(&mut child));
    }

    #[test]
    fn ed_undo_redo_roundtrip() {
        let (mut app, dir) = tmp_app("undo-roundtrip");
        let lib = dir.join("Library");
        crate::library::Library::create_group(&lib, std::path::Path::new(""), "G").unwrap();
        std::fs::write(lib.join("G").join("S.md"), "# S\n\n").unwrap();
        app.library = crate::library::Library::scan(&lib);
        app.open_selected_sheet();
        let end = app.editor.as_ref().unwrap().rope.len_chars();
        app.editor.as_mut().unwrap().cursor = end;
        app.ed_insert_char('x');
        app.ed_insert_char('y');
        assert_eq!(app.editor.as_ref().unwrap().rope.to_string(), "# S\n\nxy");
        // One burst, one undo.
        app.ed_undo();
        assert_eq!(app.editor.as_ref().unwrap().rope.to_string(), "# S\n\n");
        assert!(app.editor.as_ref().unwrap().dirty);
        app.ed_redo();
        assert_eq!(app.editor.as_ref().unwrap().rope.to_string(), "# S\n\nxy");
        // Exhausting the stack reports it instead of silently doing nothing.
        // (The redo above put the burst back, so two undos are needed.)
        app.ed_undo();
        app.ed_undo();
        assert_eq!(app.status, "nothing to undo");
        app.ed_redo();
        app.ed_redo();
        app.ed_redo();
        assert_eq!(app.status, "nothing to redo");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ed_undo_redo_multiview_segment() {
        let (mut app, dir) = tmp_app("undo-mv");
        let lib = dir.join("Library");
        crate::library::Library::create_group(&lib, std::path::Path::new(""), "G").unwrap();
        std::fs::write(lib.join("G").join("A.md"), "# A\n").unwrap();
        std::fs::write(lib.join("G").join("B.md"), "# B\n").unwrap();
        app.library = crate::library::Library::scan(&lib);
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        app.sheet_cursor = 1;
        app.toggle_mark_sheet();
        assert!(app.multiview.is_some());
        // Type in the first segment, undo it there.
        app.ed_insert_char('q');
        assert!(app.multiview.as_ref().unwrap().segments[0]
            .rope
            .to_string()
            .starts_with("q# A"));
        app.ed_undo();
        assert!(app.multiview.as_ref().unwrap().segments[0]
            .rope
            .to_string()
            .starts_with("# A"));
        // Second segment untouched throughout.
        assert!(app.multiview.as_ref().unwrap().segments[1]
            .rope
            .to_string()
            .starts_with("# B"));
        app.ed_redo();
        assert!(app.multiview.as_ref().unwrap().segments[0]
            .rope
            .to_string()
            .starts_with("q# A"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_needs_an_open_sheet() {
        let (mut app, dir) = tmp_app("find-empty-lib");
        // Nothing open: prompt must not appear.
        app.start_search();
        assert!(app.search_prompt.is_none());
        assert!(app.status.contains("nothing open"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_searches_active_multiview_segment_only() {
        let (mut app, dir) = tmp_app("find-mv");
        let lib = dir.join("Library");
        Library::create_group(&lib, std::path::Path::new(""), "G").unwrap();
        std::fs::write(lib.join("G").join("One.md"), "# One\n\napple banana\n").unwrap();
        std::fs::write(lib.join("G").join("Two.md"), "# Two\n\nbanana cherry\n").unwrap();
        app.library = Library::scan(&lib);
        app.lib_cursor = 0;
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        app.sheet_cursor = 1;
        app.toggle_mark_sheet();
        assert!(app.multiview.is_some());
        // Active segment is the first marked sheet: only its match counts.
        app.start_search();
        assert!(app.search_prompt.is_some());
        app.search_prompt.as_mut().unwrap().input = "banana".to_string();
        app.advance_search();
        assert_eq!(app.multiview.as_ref().unwrap().cursor, (0, 13));
        assert_eq!(app.status, "match 1 of 1");
        // Moving to the other segment searches that rope instead.
        app.multiview.as_mut().unwrap().cursor = (1, 0);
        app.advance_search();
        assert_eq!(app.multiview.as_ref().unwrap().cursor, (1, 7));
        assert_eq!(app.status, "match 1 of 1");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
