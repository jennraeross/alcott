use std::path::{Path, PathBuf};

use crate::words::{count_words_rope, is_word_char};

/// One reversible rope edit, addressed in document char offsets.
/// Undo applies the inverse; redo re-applies the op itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoOp {
    /// Text inserted at `pos` (undo removes it).
    Insert { pos: usize, text: String },
    /// Text removed from `pos` (undo reinserts it).
    Delete { pos: usize, text: String },
}

/// Bounded undo/redo stacks with typing-burst coalescing: adjacent inserts
/// merge into one step, as do adjacent same-direction deletes. Anything
/// involving a line break starts a fresh step so structural edits stay
/// individually undoable.
#[derive(Debug, Default)]
pub struct History {
    undo: Vec<UndoOp>,
    redo: Vec<UndoOp>,
}

impl History {
    const CAP: usize = 500;

    fn push(&mut self, op: UndoOp) {
        if self.undo.len() >= Self::CAP {
            self.undo.remove(0);
        }
        self.undo.push(op);
        self.redo.clear();
    }

    /// Record inserted text. Set `coalesce` for plain typing; newlines and
    /// other structural inserts always start a fresh step.
    pub fn record_insert(&mut self, pos: usize, text: String, coalesce: bool) {
        if coalesce && !text.contains('\n') {
            if let Some(UndoOp::Insert { pos: ppos, text: ptext }) = self.undo.last_mut() {
                if !ptext.contains('\n') && *ppos + ptext.chars().count() == pos {
                    ptext.push_str(&text);
                    self.redo.clear();
                    return;
                }
            }
        }
        self.push(UndoOp::Insert { pos, text });
    }

    /// Record removed text. `grows_left` merges backspace runs (the removed
    /// block extends leftward); otherwise merges only exact repeats at the
    /// same position (delete-forward runs). Line breaks never coalesce.
    pub fn record_delete(&mut self, pos: usize, text: String, grows_left: bool) {
        if !text.contains('\n') {
            if grows_left {
                if let Some(UndoOp::Delete { pos: ppos, text: ptext }) = self.undo.last_mut() {
                    if !ptext.contains('\n') && pos + text.chars().count() == *ppos {
                        let mut merged = text;
                        merged.push_str(ptext);
                        *ppos = pos;
                        *ptext = merged;
                        self.redo.clear();
                        return;
                    }
                }
            } else if let Some(UndoOp::Delete { pos: ppos, text: ptext }) = self.undo.last_mut() {
                if !ptext.contains('\n') && pos == *ppos {
                    ptext.push_str(&text);
                    self.redo.clear();
                    return;
                }
            }
        }
        self.push(UndoOp::Delete { pos, text });
    }

    /// Undo one step against `rope`, moving `cursor` to where continued
    /// editing feels natural (original position for backspace/typing,
    /// end of restored text for deletions). Returns false when empty.
    pub fn undo(&mut self, rope: &mut ropey::Rope, cursor: &mut usize) -> bool {
        let Some(op) = self.undo.pop() else {
            return false;
        };
        match op {
            UndoOp::Insert { pos, text } => {
                let len = text.chars().count();
                let pos = pos.min(rope.len_chars());
                rope.remove(pos..(pos + len).min(rope.len_chars()));
                *cursor = pos;
                self.redo.push(UndoOp::Insert { pos, text });
            }
            UndoOp::Delete { pos, text } => {
                let pos = pos.min(rope.len_chars());
                rope.insert(pos, &text);
                *cursor = (pos + text.chars().count()).min(rope.len_chars());
                self.redo.push(UndoOp::Delete { pos, text });
            }
        }
        true
    }

    /// Redo one undone step. Returns false when empty.
    pub fn redo(&mut self, rope: &mut ropey::Rope, cursor: &mut usize) -> bool {
        let Some(op) = self.redo.pop() else {
            return false;
        };
        match op {
            UndoOp::Insert { pos, text } => {
                let pos = pos.min(rope.len_chars());
                rope.insert(pos, &text);
                *cursor = (pos + text.chars().count()).min(rope.len_chars());
                self.undo.push(UndoOp::Insert { pos, text });
            }
            UndoOp::Delete { pos, text } => {
                let len = text.chars().count();
                let pos = pos.min(rope.len_chars());
                rope.remove(pos..(pos + len).min(rope.len_chars()));
                *cursor = pos;
                self.undo.push(UndoOp::Delete { pos, text });
            }
        }
        true
    }
}

pub struct SheetBuffer {
    pub path: PathBuf,
    pub rope: ropey::Rope,
    /// char index
    pub cursor: usize,
    pub dirty: bool,
    pub history: History,
}

impl SheetBuffer {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let rope = ropey::Rope::from_str(&text);
        let cursor = 0;
        Ok(Self {
            path: path.to_path_buf(),
            rope,
            cursor,
            dirty: false,
            history: History::default(),
        })
    }

    /// Full buffer text (used by tests; hot paths read the rope directly).
    #[allow(dead_code)]
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn words(&self) -> usize {
        count_words_rope(&self.rope)
    }

    /// Insert char at cursor. Returns true if a word was completed
    /// (prev char was word-char, new char is a boundary) for gross_typed.
    pub fn insert_char(&mut self, c: char) -> bool {
        let pos = self.cursor.min(self.rope.len_chars());
        let done = insert_char_at(&mut self.rope, &mut self.cursor, c);
        self.history.record_insert(pos, c.to_string(), true);
        self.dirty = true;
        done
    }

    pub fn insert_newline(&mut self) -> bool {
        let line_idx = self.rope.char_to_line(self.cursor.min(self.rope.len_chars()));
        let line: String = self.rope.line(line_idx).to_string();
        let pos = self.cursor.min(self.rope.len_chars());
        let done = insert_newline_at(&mut self.rope, &mut self.cursor, &line);
        let text: String = self.rope.slice(pos..self.cursor).chars().collect();
        self.history.record_insert(pos, text, false);
        self.dirty = true;
        done
    }

    /// Atomic write (temp file + rename) shared with multi-view segments.
    pub fn save(&mut self) -> std::io::Result<()> {
        atomic_write(&self.path, &self.rope.to_string())?;
        self.dirty = false;
        Ok(())
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        if let Some(ch) = self.rope.get_char(self.cursor) {
            self.history
                .record_delete(self.cursor, ch.to_string(), true);
            self.rope.remove(self.cursor..self.cursor + 1);
        }
        self.dirty = true;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.rope.len_chars() {
            return;
        }
        if let Some(ch) = self.rope.get_char(self.cursor) {
            self.history
                .record_delete(self.cursor, ch.to_string(), false);
            self.rope.remove(self.cursor..self.cursor + 1);
        }
        self.dirty = true;
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }
    pub fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.rope.len_chars());
    }
    /// Up one visual (wrapped) row; falls back to a rope-line hop when the
    /// viewport width is unknown (0).
    pub fn move_up_visual(&mut self, width: u16) {
        let c = self.cursor.min(self.rope.len_chars());
        let line = self.rope.char_to_line(c);
        let text = crate::view::strip_newline(&self.rope.line(line).to_string()).to_string();
        self.cursor = crate::view::visual_up(&self.rope, c, &text, width);
    }
    /// Down one visual (wrapped) row; falls back to a rope-line hop when the
    /// viewport width is unknown (0).
    pub fn move_down_visual(&mut self, width: u16) {
        let c = self.cursor.min(self.rope.len_chars());
        let line = self.rope.char_to_line(c);
        let text = crate::view::strip_newline(&self.rope.line(line).to_string()).to_string();
        self.cursor = crate::view::visual_down(&self.rope, c, &text, width);
    }
    pub fn move_home(&mut self) {
        let line = self.rope.char_to_line(self.cursor.min(self.rope.len_chars()));
        self.cursor = self.rope.line_to_char(line);
    }
    pub fn move_end(&mut self) {
        let line = self.rope.char_to_line(self.cursor.min(self.rope.len_chars()));
        let start = self.rope.line_to_char(line);
        let len = self.rope.line(line).len_chars();
        let mut end = start + len;
        // don't sit on the newline
        if end > start && self.rope.get_char(end - 1) == Some('\n') {
            end -= 1;
        }
        self.cursor = end.min(self.rope.len_chars());
    }

    /// Delete the whole logical line under the cursor (Edit menu: K).
    pub fn delete_current_line(&mut self) {
        if self.rope.len_chars() == 0 {
            return;
        }
        let line = self.rope.char_to_line(self.cursor.min(self.rope.len_chars()));
        let start = self.rope.line_to_char(line);
        let len = self.rope.line(line).len_chars();
        if len == 0 {
            return;
        }
        let text: String = self.rope.slice(start..start + len).chars().collect();
        self.history.record_delete(start, text, false);
        // Whole-line deletes stay separate undo steps.
        self.rope.remove(start..start + len);
        self.cursor = start.min(self.rope.len_chars());
        self.dirty = true;
    }

    /// Undo one step. Returns false when the stack is empty.
    pub fn undo(&mut self) -> bool {
        if self.history.undo(&mut self.rope, &mut self.cursor) {
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Redo one undone step. Returns false when the stack is empty.
    pub fn redo(&mut self) -> bool {
        if self.history.redo(&mut self.rope, &mut self.cursor) {
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn cursor_line_col(&self) -> (usize, usize) {
        let c = self.cursor.min(self.rope.len_chars());
        let line = self.rope.char_to_line(c);
        let col = c - self.rope.line_to_char(line);
        (line, col)
    }
}

/// Atomic write (temp file + rename).
pub fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Auto-continue prefix for a newline: indent + list/quote marker.
/// Numbers increment (`1.` → `2.`, `9)` → `10)`).
pub fn newline_prefix_for(line: &str) -> String {
    let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
    let trimmed = line.trim_start();
    let mut prefix = indent;
    // blockquote
    if let Some(rest) = trimmed.strip_prefix('>') {
        prefix.push('>');
        if rest.starts_with(' ') {
            prefix.push(' ');
        }
    } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
        prefix.push_str(&trimmed[..2]);
    } else {
        // Ordered list: continue with the next number, keeping `.`/`)`.
        let bytes = trimmed.as_bytes();
        let mut digits = 0;
        while digits < bytes.len() && bytes[digits].is_ascii_digit() {
            digits += 1;
        }
        if digits > 0
            && digits < bytes.len()
            && (bytes[digits] == b'.' || bytes[digits] == b')')
            && bytes.get(digits + 1) == Some(&b' ')
        {
            let n: u64 = trimmed[..digits].parse().unwrap_or(0);
            prefix.push_str(&(n + 1).to_string());
            prefix.push(bytes[digits] as char);
            prefix.push(' ');
        }
    }
    prefix
}

/// Insert `\n` + auto-continue prefix at `*cursor`. Returns true when a word
/// was completed (for gross word stats).
pub fn insert_newline_at(rope: &mut ropey::Rope, cursor: &mut usize, line: &str) -> bool {
    let prefix = newline_prefix_for(line);
    let prev_is_word = if *cursor > 0 {
        rope.get_char(cursor.saturating_sub(1))
            .map(is_word_char)
            .unwrap_or(false)
    } else {
        false
    };
    let ins = format!("\n{prefix}");
    rope.insert(*cursor, &ins);
    *cursor += ins.chars().count();
    prev_is_word
}

/// Insert char at `*cursor`. Returns true on word completion.
pub fn insert_char_at(rope: &mut ropey::Rope, cursor: &mut usize, c: char) -> bool {
    let prev_is_word = if *cursor > 0 {
        rope.get_char(cursor.saturating_sub(1))
            .map(is_word_char)
            .unwrap_or(false)
    } else {
        false
    };
    rope.insert_char(*cursor, c);
    *cursor += 1;
    prev_is_word && !is_word_char(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buf_at_end(text: &str) -> SheetBuffer {
        let rope = ropey::Rope::from_str(text);
        let cursor = rope.len_chars();
        SheetBuffer {
            path: std::path::PathBuf::from("t.md"),
            rope,
            cursor,
            dirty: false,
            history: History::default(),
        }
    }

    #[test]
    fn numbered_list_increments() {
        let mut b = buf_at_end("1. apple");
        b.insert_newline();
        assert_eq!(b.text(), "1. apple\n2. ");
    }

    #[test]
    fn numbered_list_paren_increments() {
        let mut b = buf_at_end("  9) apple");
        b.insert_newline();
        assert_eq!(b.text(), "  9) apple\n  10) ");
    }

    #[test]
    fn bullet_list_unchanged() {
        let mut b = buf_at_end("- apple");
        b.insert_newline();
        assert_eq!(b.text(), "- apple\n- ");
    }

    #[test]
    fn undo_redo_typing_burst_is_one_step() {
        let mut b = buf_at("", 0);
        b.insert_char('h');
        b.insert_char('i');
        assert_eq!(b.text(), "hi");
        assert!(b.undo());
        assert_eq!(b.text(), "");
        assert_eq!(b.cursor, 0);
        assert!(b.redo());
        assert_eq!(b.text(), "hi");
        assert_eq!(b.cursor, 2);
    }

    #[test]
    fn undo_backspace_run_is_one_step() {
        let mut b = buf_at("abcd", 4);
        b.backspace();
        b.backspace();
        assert_eq!(b.text(), "ab");
        assert!(b.undo());
        assert_eq!(b.text(), "abcd");
        assert_eq!(b.cursor, 4);
    }

    #[test]
    fn newline_breaks_undo_coalescing() {
        let mut b = buf_at("", 0);
        b.insert_char('a');
        b.insert_newline();
        b.insert_char('b');
        assert_eq!(b.text(), "a\nb");
        // 'b' undoes alone (newline sealed the burst before it).
        assert!(b.undo());
        assert_eq!(b.text(), "a\n");
        // The bare newline undoes alone too.
        assert!(b.undo());
        assert_eq!(b.text(), "a");
        assert!(b.undo());
        assert_eq!(b.text(), "");
        assert!(!b.undo());
    }

    #[test]
    fn redo_cleared_by_new_edit() {
        let mut b = buf_at("", 0);
        b.insert_char('a');
        assert!(b.undo());
        assert_eq!(b.text(), "");
        b.insert_char('b');
        assert!(!b.redo());
        assert_eq!(b.text(), "b");
    }

    #[test]
    fn undo_delete_current_line_restores_it() {
        let mut b = buf_at("aa\nbb\n", 0);
        b.delete_current_line();
        assert_eq!(b.text(), "bb\n");
        assert!(b.undo());
        assert_eq!(b.text(), "aa\nbb\n");
        assert!(b.redo());
        assert_eq!(b.text(), "bb\n");
    }

    #[test]
    fn history_is_capped() {
        let mut b = buf_at("", 0);
        for _ in 0..(crate::buffer::History::CAP + 50) {
            b.insert_newline();
        }
        assert!(b.history.undo.len() <= crate::buffer::History::CAP);
    }

    fn buf_at(text: &str, cursor: usize) -> SheetBuffer {
        SheetBuffer {
            path: std::path::PathBuf::from("t.md"),
            rope: ropey::Rope::from_str(text),
            cursor,
            dirty: false,
            history: History::default(),
        }
    }

    #[test]
    fn visual_up_down_steps_wrapped_rows() {
        // "abcdefghij klmnopqrst": two 10-col visual rows at width 10
        // (chars 0-9, then 11-20; the space at 10 is wrap-dropped).
        let text = "abcdefghij klmnopqrst\nshort\n";
        let mut b = buf_at(text, 21);
        // End of line 0, second visual row: Up lands at end of row 0.
        b.move_up_visual(10);
        assert_eq!(b.cursor, 10);
        // Top of the document.
        b.move_up_visual(10);
        assert_eq!(b.cursor, 0);
        // Down from (0,0) lands on the second visual row, same column.
        b.move_down_visual(10);
        assert_eq!(b.cursor, 11);
        // Down from the last row of line 0 drops to line 1, column clamped.
        b.move_down_visual(10);
        let l1 = b.rope.line_to_char(1);
        assert_eq!(b.cursor, l1 + 5);
        // Down past the end clamps to EOF.
        b.cursor = b.rope.len_chars();
        b.move_down_visual(10);
        assert_eq!(b.cursor, b.rope.len_chars());
    }

    #[test]
    fn visual_up_down_falls_back_without_width() {
        let mut b = buf_at("ab\ncdef\n", 4); // line 1, col 1
        b.move_up_visual(0);
        assert_eq!(b.cursor, 1); // line 0, col 1
        b.move_down_visual(0);
        assert_eq!(b.cursor, 4);
    }
}
