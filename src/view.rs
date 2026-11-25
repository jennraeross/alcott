//! Viewport-windowed editor rendering support.
//!
//! The editor used to style + wrap the whole document every frame
//! (`style_doc` over all lines, then `Paragraph` with `Wrap`). On a 46KB
//! chapter that is tens of milliseconds per keystroke. This module ports
//! ratatui's `WordWrapper` (private `reflow` module, `trim = false` — the
//! exact algorithm `Paragraph` applied) so the editor can:
//!
//! - style + wrap only the visible window of lines,
//! - map visual rows back to rope positions (cursor, clicks, scroll),
//! - move the cursor by visual (wrapped) rows instead of rope lines.
//!
//! Wrap geometry depends only on grapheme widths and whitespace, never on
//! styles, so cursor math can reuse the same machine over plain text.

use std::collections::VecDeque;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

const NBSP: &str = "\u{00a0}";
const ZWSP: &str = "\u{200b}";

/// Mirrors ratatui's private `StyledGrapheme::is_whitespace`.
fn is_wrap_ws(symbol: &str) -> bool {
    symbol == ZWSP || (symbol != NBSP && symbol.chars().all(|c| c.is_whitespace()))
}

struct G<'a> {
    sym: &'a str,
    style: Style,
    nchars: usize,
    width: u16,
}

/// One wrapped visual row of a styled line.
pub struct WrappedRow {
    pub spans: Vec<Span<'static>>,
    /// Rope-line index in the caller's slice.
    pub line: usize,
    /// Char offset in the rope line where this row starts.
    pub char_start: usize,
    /// Char count covered by this row.
    pub char_len: usize,
}

/// Wrap pre-styled lines exactly the way `Paragraph` with
/// `Wrap { trim: false }` would. `line_no[i]` tags every row produced
/// from `lines[i]` for caller-side mapping.
pub fn wrap_styled(lines: &[Line<'static>], line_no: &[usize], width: u16) -> Vec<WrappedRow> {
    let mut rows = Vec::new();
    if width == 0 {
        return rows;
    }
    for (li, line) in lines.iter().enumerate() {
        // Same grapheme stream Paragraph feeds its composer: each span's
        // graphemes with the paragraph base style (default) patched in.
        let mut gs: Vec<G> = Vec::new();
        for span in &line.spans {
            for g in span.styled_graphemes(Style::default()) {
                gs.push(G {
                    sym: g.symbol,
                    style: g.style,
                    nchars: g.symbol.chars().count(),
                    width: g.symbol.width() as u16,
                });
            }
        }
        wrap_graphemes(&gs, line_no.get(li).copied().unwrap_or(li), width, &mut rows);
    }
    rows
}

/// Geometry-only wrap of one plain-text line (no trailing newline).
/// Returns `(char_start, char_len)` per visual row. Break decisions are
/// identical to [`wrap_styled`] because styles never affect geometry.
pub fn wrap_geometry(line_text: &str, width: u16) -> Vec<(usize, usize)> {
    if width == 0 {
        return Vec::new();
    }
    let styled = crate::md_style::style_line(line_text);
    let mut gs: Vec<G> = Vec::new();
    for span in &styled.spans {
        for g in span.styled_graphemes(Style::default()) {
            gs.push(G {
                sym: g.symbol,
                style: g.style,
                nchars: g.symbol.chars().count(),
                width: g.symbol.width() as u16,
            });
        }
    }
    let mut rows: Vec<WrappedRow> = Vec::new();
    wrap_graphemes(&gs, 0, width, &mut rows);
    rows.into_iter().map(|r| (r.char_start, r.char_len)).collect()
}

/// Column (terminal cells) → char offset within `text`, for click mapping.
/// Zero-width chars attach to the current cell; a wide char straddling `x`
/// selects the char containing `x`.
pub fn col_to_char(text: &str, x: usize) -> usize {
    let mut col = 0usize;
    for (i, c) in text.char_indices() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if col + w > x {
            return text[..i].chars().count();
        }
        col += w;
    }
    text.chars().count()
}

/// Display width (terminal cells) of the first `n` chars of `text`.
pub fn prefix_width(text: &str, n: usize) -> u16 {
    let mut w = 0usize;
    for c in text.chars().take(n) {
        w += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
    }
    w.min(u16::MAX as usize) as u16
}

/// Render pre-wrapped rows verbatim: the exact cell writes
/// `Paragraph::render_text` performs (Left-aligned, no scroll offset), with
/// no wrapping and no truncation. Rows are already wrapped to fit; a wide
/// glyph may visually overhang the last cell exactly the way the wrapped
/// `Paragraph` path renders it.
pub fn render_rows(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, rows: &[Line<'static>]) {
    use ratatui::style::Style;
    let buf = frame.buffer_mut();
    let mut y = 0u16;
    for line in rows {
        if y >= area.height {
            break;
        }
        let mut x = 0u16;
        for span in &line.spans {
            for g in span.styled_graphemes(Style::default()) {
                let w = g.symbol.width() as u16;
                if w == 0 {
                    continue;
                }
                let symbol = if g.symbol.is_empty() { " " } else { g.symbol };
                buf[(area.left() + x, area.top() + y)]
                    .set_symbol(symbol)
                    .set_style(g.style);
                x += w;
            }
        }
        y += 1;
    }
}

/// Strip one trailing `\n` (plus an optional preceding `\r`) for display and
/// wrap geometry. Columns never address the line break itself.
pub fn strip_newline(s: &str) -> &str {
    s.strip_suffix('\n')
        .map(|t| t.strip_suffix('\r').unwrap_or(t))
        .unwrap_or(s)
}

/// Move the cursor up one *visual* (wrapped) row. Falls back to a rope-line
/// hop when the width is unknown (0). `line_text` is the current rope line
/// without its line break; returns the new char index.
pub fn visual_up(
    rope: &ropey::Rope,
    cursor: usize,
    line_text: &str,
    width: u16,
) -> usize {
    let cursor = cursor.min(rope.len_chars());
    let line = rope.char_to_line(cursor);
    if width == 0 {
        return rope_up_line(rope, cursor, line);
    }
    let col = cursor.saturating_sub(rope.line_to_char(line));
    let rows = wrap_geometry(line_text, width);
    let mut row = rows.len().saturating_sub(1);
    for (i, (start, _)) in rows.iter().enumerate() {
        if *start > col {
            break;
        }
        row = i;
    }
    // Clamp the column into the row (cursor may sit on the newline).
    let (rs, rl) = rows.get(row).copied().unwrap_or((0, 0));
    let col = col.min(rs + rl);
    if row > 0 {
        // Same-line hop: keep the visual column relative to the row start,
        // clamped into the target row.
        let (ps, pl) = rows[row - 1];
        let rel = col.saturating_sub(rs);
        return rope.line_to_char(line) + ps + rel.min(pl);
    }
    if line == 0 {
        return 0;
    }
    let owned = rope.line(line - 1).to_string();
    let prev = strip_newline(&owned);
    let prows = wrap_geometry(prev, width);
    let (ps, pl) = prows.last().copied().unwrap_or((0, 0));
    rope.line_to_char(line - 1) + col.min(ps + pl)
}

/// Move the cursor down one *visual* (wrapped) row. Mirror of [`visual_up`].
pub fn visual_down(
    rope: &ropey::Rope,
    cursor: usize,
    line_text: &str,
    width: u16,
) -> usize {
    let cursor = cursor.min(rope.len_chars());
    let line = rope.char_to_line(cursor);
    if width == 0 {
        return rope_down_line(rope, cursor, line);
    }
    let col = cursor.saturating_sub(rope.line_to_char(line));
    let rows = wrap_geometry(line_text, width);
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
        return rope.line_to_char(line) + ns + rel.min(nl);
    }
    if line + 1 >= rope.len_lines() {
        return rope.len_chars();
    }
    let owned = rope.line(line + 1).to_string();
    let next = strip_newline(&owned);
    let nrows = wrap_geometry(next, width);
    let (ns, nl) = nrows.first().copied().unwrap_or((0, 0));
    rope.line_to_char(line + 1) + col.min(ns + nl)
}

fn rope_up_line(rope: &ropey::Rope, cursor: usize, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let col = cursor.saturating_sub(rope.line_to_char(line));
    let prev_start = rope.line_to_char(line - 1);
    let mut prev_len = rope.line(line - 1).len_chars();
    if prev_len > 0 && rope.get_char(prev_start + prev_len - 1) == Some('\n') {
        prev_len -= 1;
    }
    prev_start + col.min(prev_len)
}

fn rope_down_line(rope: &ropey::Rope, cursor: usize, line: usize) -> usize {
    if line + 1 >= rope.len_lines() {
        return rope.len_chars();
    }
    let col = cursor.saturating_sub(rope.line_to_char(line));
    let next_start = rope.line_to_char(line + 1);
    let mut next_len = rope.line(line + 1).len_chars();
    if next_len > 0 && rope.get_char(next_start + next_len - 1) == Some('\n') {
        next_len -= 1;
    }
    next_start + col.min(next_len)
}

/// Faithful port of ratatui 0.29 `WordWrapper::process_input` with
/// `trim = false`, operating on pre-collected graphemes. Appends this
/// line's visual rows to `rows`.
fn wrap_graphemes(gs: &[G], line_no: usize, width: u16, rows: &mut Vec<WrappedRow>) {
    // Char prefix sums for row char ranges.
    let mut coff: Vec<usize> = Vec::with_capacity(gs.len() + 1);
    coff.push(0);
    for g in gs {
        coff.push(coff.last().copied().unwrap_or(0) + g.nchars);
    }
    let emit = |idxs: &[usize], fallback: usize, rows: &mut Vec<WrappedRow>| {
        fn flush(cur: &mut Option<(Style, String)>, spans: &mut Vec<Span<'static>>) {
            if let Some((style, text)) = cur.take() {
                if !text.is_empty() || spans.is_empty() {
                    spans.push(Span::styled(text, style));
                }
            }
        }
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut cur: Option<(Style, String)> = None;
        for &k in idxs {
            let st = gs[k].style;
            match cur {
                Some((s, ref mut t)) if s == st => t.push_str(gs[k].sym),
                _ => {
                    flush(&mut cur, &mut spans);
                    cur = Some((st, gs[k].sym.to_string()));
                }
            }
        }
        flush(&mut cur, &mut spans);
        let (char_start, char_len) = match (idxs.first(), idxs.last()) {
            (Some(&a), Some(&b)) => (coff[a], coff[b + 1] - coff[a]),
            // Zero-width marker row: anchor at the scan position.
            _ => (fallback, 0usize),
        };
        rows.push(WrappedRow {
            spans,
            line: line_no,
            char_start,
            char_len,
        });
    };

    let mut pending_line: Vec<usize> = Vec::new();
    let mut pending_word: Vec<usize> = Vec::new();
    let mut pending_ws: VecDeque<usize> = VecDeque::new();
    let mut line_width: u16 = 0;
    let mut word_width: u16 = 0;
    let mut ws_width: u16 = 0;
    let mut non_ws_prev = false;

    let rows_before = rows.len();
    for (i, g) in gs.iter().enumerate() {
        let ws = is_wrap_ws(g.sym);
        let sw = g.width;
        // Symbols wider than the line are dropped entirely (and do not
        // update the word-boundary tracker).
        if sw > width {
            continue;
        }
        let word_found = non_ws_prev && ws;
        // trim = false: only the untrimmed overflow applies.
        let untrimmed_overflow =
            pending_line.is_empty() && word_width + ws_width + sw > width;
        if word_found || untrimmed_overflow {
            for &k in &pending_ws {
                pending_line.push(k);
            }
            pending_ws.clear();
            line_width += ws_width;
            pending_line.append(&mut pending_word);
            line_width += word_width;
            ws_width = 0;
            word_width = 0;
        }
        let line_full = line_width >= width;
        let word_overflow = sw > 0 && line_width + ws_width + word_width >= width;
        if line_full || word_overflow {
            let mut remaining = width.saturating_sub(line_width);
            emit(&pending_line, coff[i], rows);
            pending_line.clear();
            line_width = 0;
            while let Some(&k) = pending_ws.front() {
                let w = gs[k].width;
                if w > remaining {
                    break;
                }
                ws_width -= w;
                remaining -= w;
                pending_ws.pop_front();
            }
            if ws && pending_ws.is_empty() {
                continue;
            }
        }
        if ws {
            ws_width += sw;
            pending_ws.push_back(i);
        } else {
            word_width += sw;
            pending_word.push(i);
        }
        non_ws_prev = !ws;
    }

    // End of line (mirrors upstream exactly).
    if pending_line.is_empty() && pending_word.is_empty() && !pending_ws.is_empty() {
        // Whitespace-only remainder: an empty marker row, then the spaces.
        let at = coff.get(gs.len().saturating_sub(pending_ws.len())).copied().unwrap_or(0);
        rows.push(WrappedRow {
            spans: Vec::new(),
            line: line_no,
            char_start: at,
            char_len: 0,
        });
    }
    // trim = false: whitespace always joins the pending line.
    for k in pending_ws.drain(..) {
        pending_line.push(k);
    }
    pending_line.append(&mut pending_word);
    if !pending_line.is_empty() {
        emit(&pending_line, coff[gs.len()], rows);
    }
    if rows.len() == rows_before {
        // Empty input (or every grapheme dropped as wider than the area):
        // upstream still yields exactly one blank row for the line.
        rows.push(WrappedRow {
            spans: Vec::new(),
            line: line_no,
            char_start: coff[gs.len()],
            char_len: 0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Rect;
    use ratatui::widgets::{Paragraph, Wrap};
    use ratatui::Terminal;

    fn sample_doc() -> Vec<Line<'static>> {
        let text = [
            "# Heading **bold** and *italic*",
            "",
            "It was on a dreary night of November, that I beheld the accomplishment of my toils. With an anxiety that almost amounted to agony, I collected the instruments of life around me.",
            "   indented verse line one",
            "     indented verse line two with more words to force wrapping behavior here",
            "    ",
            "- list item with `code` and a [link](https://example.com) inside",
            "1. ordered item __underlined__",
            "> a quote that goes on long enough to wrap across several visual rows at narrow widths",
            "```rust",
            "let x = 1; // code block content that should all fit",
            "```",
            "averylongwordthatexceedsthewidthbyfarandmustbebrokenhardtowrapit",
            "tabs\there and  multiple   spaces   inside",
            "CJK混在テキスト mixed with ascii words to test double width wrapping",
            "trailing spaces   ",
            "[ref]: https://example.com",
            "see [ref] shortcut",
        ]
        .join("\n");
        crate::md_style::style_doc(&text)
    }

    fn render_old(doc: Vec<Line<'static>>, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut term = Terminal::new(backend).unwrap();
        term.draw(|f| {
            f.render_widget(
                Paragraph::new(doc).wrap(Wrap { trim: false }),
                f.area(),
            );
        })
        .unwrap();
        term.backend().buffer().clone()
    }

    /// Widget that writes pre-wrapped rows verbatim: the exact cell writes
    /// `Paragraph::render_text` performs (minus scroll/alignment, which are
    /// Left/zero here), with no wrapping and no truncation. Lets the parity
    /// test run both paths through the same Terminal+diff pipeline.
    struct RawLines<'a> {
        lines: Vec<Line<'a>>,
    }

    impl<'a> ratatui::widgets::Widget for RawLines<'a> {
        fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
            use ratatui::style::Style;
            use unicode_width::UnicodeWidthStr;
            let mut y = 0u16;
            for line in &self.lines {
                if y >= area.height {
                    break;
                }
                let mut x = 0u16;
                for span in &line.spans {
                    for g in span.styled_graphemes(Style::default()) {
                        let w = g.symbol.width() as u16;
                        if w == 0 {
                            continue;
                        }
                        let symbol = if g.symbol.is_empty() { " " } else { g.symbol };
                        buf[(area.left() + x, area.top() + y)].set_symbol(symbol).set_style(g.style);
                        x += w;
                    }
                }
                y += 1;
            }
        }
    }

    fn render_new(doc: &[Line<'static>], width: u16, height: u16) -> ratatui::buffer::Buffer {
        use ratatui::layout::Rect;
        let nos: Vec<usize> = (0..doc.len()).collect();
        let rows = wrap_styled(doc, &nos, width);
        let lines: Vec<Line<'static>> = rows.into_iter().map(|r| Line::from(r.spans)).collect();
        let backend = TestBackend::new(width, height);
        let mut term = Terminal::new(backend).unwrap();
        let area = Rect::new(0, 0, width, height);
        term.draw(|f| {
            f.render_widget(RawLines { lines }, area);
        })
        .unwrap();
        term.backend().buffer().clone()
    }

    #[test]
    fn wrap_matches_paragraph_wrap() {
        let doc = sample_doc();
        // Upper bound on visual rows: every char could start a row.
        let total_chars: usize = doc
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.len()).sum::<usize>())
            .sum();
        let height = (total_chars + doc.len() + 8).min(1500) as u16;
        for width in [3u16, 4, 5, 6, 7, 8, 10, 12, 20, 40, 80] {
            let old = render_old(doc.clone(), width, height);
            let new = render_new(&doc, width, height);
            assert_eq!(old.area, new.area, "area at width {width}");
            if old.content != new.content {
                let dump = |buf: &ratatui::buffer::Buffer| -> Vec<String> {
                    (0..height)
                        .map(|y| {
                            (0..width)
                                .map(|x| buf[(x, y)].symbol().to_string())
                                .collect::<String>()
                        })
                        .collect()
                };
                let (o, n) = (dump(&old), dump(&new));
                for (y, (a, b)) in o.iter().zip(n.iter()).enumerate() {
                    if a != b {
                        let lo = y.saturating_sub(2);
                        let mut msg = format!("width {width} first diff at row {y}:\n");
                        for z in lo..y + 8 {
                            let ac: Vec<String> = (0..width)
                                .map(|x| {
                                    let c = &old.content[z * (width as usize) + (x as usize)];
                                    format!("{:?}", c.symbol())
                                })
                                .collect();
                            let bc: Vec<String> = (0..width)
                                .map(|x| {
                                    let c = &new.content[z * (width as usize) + (x as usize)];
                                    format!("{:?}", c.symbol())
                                })
                                .collect();
                            msg.push_str(&format!("  {z}:\n    old {ac:?}\n    new {bc:?}\n"));
                        }
                        panic!("{msg}");
                    }
                }
                panic!("width {width}: same rows, different styles?");
            }
        }
    }

    #[test]
    fn geometry_covers_every_char() {
        // Every non-whitespace char of every line belongs to exactly one
        // row (wrap-dropped whitespace belongs to no row).
        let doc = sample_doc();
        for width in [8u16, 40] {
            let nos: Vec<usize> = (0..doc.len()).collect();
            let rows = wrap_styled(&doc, &nos, width);
            for (li, line) in doc.iter().enumerate() {
                let text: String =
                    line.spans.iter().map(|s| s.content.as_ref()).collect();
                let chars: Vec<char> = text.chars().collect();
                let mut covered = vec![false; chars.len()];
                for r in rows.iter().filter(|r| r.line == li) {
                    covered[r.char_start..r.char_start + r.char_len].fill(true);
                }
                for (i, c) in chars.iter().enumerate() {
                    if !c.is_whitespace() {
                        assert!(covered[i], "char {i} ({c:?}) uncovered line {li} width {width}");
                    }
                }
            }
        }
    }
}
