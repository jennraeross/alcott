//! Basic inline Markdown preview for the editor.
//!
//! Conventions (see View > Markdown guide):
//! `**bold**`, `*italic*` / `_italic_`, `__underline__`, `` `code` ``,
//! `[text](url)` / `[text][ref]` / `![alt](src)`, `# headings`, `-`/`*`/`+`/
//! `1.` lists, `>` quotes, fenced ```` ``` ```` / `~~~` blocks. Symbols stay
//! visible (de-emphasised) so columns — and the cursor math — never shift.

use std::collections::HashSet;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::config::theme;

/// Style one buffer line (no cross-line state: no fences, no ref defs).
/// Kept for single-line contexts (tests, previews); the editor uses
/// [`style_doc`].
#[allow(dead_code)]
pub fn style_line(line: &str) -> Line<'static> {
    Line::from(style_block(line, &HashSet::new()))
}

/// Style a whole document (one [`Line`] per `\n`-separated row), tracking
/// fenced code blocks and `[ref]: url` definitions across lines.
pub fn style_doc(text: &str) -> Vec<Line<'static>> {
    // split('\n') mirrors Paragraph's own splitting (keeps trailing blank).
    let raw: Vec<&str> = text.split('\n').collect();
    // Scan refdefs once; reuse the per-line results in the style loop below.
    let refdefs: Vec<Option<String>> = raw.iter().map(|l| parse_refdef(l)).collect();
    let defs: HashSet<String> = refdefs.iter().filter_map(|o| o.clone()).collect();
    let mut out = Vec::with_capacity(raw.len());
    let mut fence: Option<char> = None;
    for (line, def) in raw.iter().zip(refdefs.iter()) {
        out.push(style_line_fenced(line, &defs, def.clone(), &mut fence));
    }
    out
}

/// Style one rope line with caller-managed block state. `def` is this line's
/// pre-scanned `[ref]: url` key (if any); `fence` is threaded across lines.
/// The windowed editor styles only the visible window this way instead of
/// re-styling the whole document every frame.
pub(crate) fn style_line_fenced(
    line: &str,
    defs: &HashSet<String>,
    def: Option<String>,
    fence: &mut Option<char>,
) -> Line<'static> {
    if let Some(f) = *fence {
        if fence_close(line, f) {
            *fence = None;
            return style_fence_line(line);
        }
        return Line::from(vec![Span::styled(
            line.to_string(),
            theme::md_code(),
        )]);
    }
    if let Some(f) = fence_open(line) {
        *fence = Some(f);
        return style_fence_line(line);
    }
    if def.is_some() {
        return Line::from(vec![Span::styled(
            line.to_string(),
            theme::md_sym(),
        )]);
    }
    Line::from(style_block(line, defs))
}

/// Collect `[ref]: url` keys from a rope without copying whole buffers:
/// only lines that can plausibly be definitions (up to 3 leading spaces
/// then `[`) are materialised for the full parse.
pub(crate) fn collect_refdefs_rope(rope: &ropey::Rope) -> HashSet<String> {
    let mut defs = HashSet::new();
    for i in 0..rope.len_lines() {
        let line = rope.line(i);
        // peek: optional ≤3 spaces then '[' (mirrors parse_refdef's prefix)
        let mut spaces = 0usize;
        let mut candidate = false;
        for c in line.chars().take(5) {
            if c == ' ' && spaces < 3 {
                spaces += 1;
                continue;
            }
            candidate = c == '[';
            break;
        }
        if !candidate {
            continue;
        }
        if let Some(key) = parse_refdef(&line.to_string()) {
            if !key.is_empty() {
                defs.insert(key);
            }
        }
    }
    defs
}

/// Fence-block state (` ``` `/`~~~`) at the start of rope line `upto`.
/// Only lines that can open/close a fence are materialised; anything else
/// provably leaves the state unchanged.
pub(crate) fn fence_state_at(rope: &ropey::Rope, upto: usize) -> Option<char> {
    let mut fence: Option<char> = None;
    for i in 0..upto.min(rope.len_lines()) {
        let line = rope.line(i);
        // peek: first non-blank char must be a tick to change state
        let mut indent = 0usize;
        let mut tick = None;
        for c in line.chars() {
            if c == ' ' {
                indent += 1;
                continue;
            }
            if c == '\t' {
                indent += 4;
                continue;
            }
            if (c == '`' || c == '~') && indent < 4 {
                tick = Some(c);
            }
            break;
        }
        let Some(t) = tick else { continue };
        let text = line.to_string();
        match fence {
            Some(f) => {
                if f == t && fence_close(&text, f) {
                    fence = None;
                }
            }
            None => {
                if let Some(f) = fence_open(&text) {
                    fence = Some(f);
                }
            }
        }
    }
    fence
}

/// Highlight the character under the cursor with reverse video.
///
/// The terminal's own cursor block can render dark-on-dark depending on the
/// user's terminal settings, so the editor draws its cursor itself (and hides
/// the terminal cursor while editing). Symbols stay in place, so columns are
/// unaffected. At end-of-line (or on an empty line) a reversed space is
/// appended as the block. `col` is a char index into the line's text.
pub fn with_cursor(line: Line<'static>, col: usize) -> Line<'static> {
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut idx = 0;
    let mut done = false;
    for span in line.spans {
        if done {
            out.push(span);
            continue;
        }
        let chars: Vec<char> = span.content.chars().collect();
        if idx + chars.len() <= col {
            idx += chars.len();
            out.push(span);
            continue;
        }
        let at = col - idx;
        let before: String = chars[..at].iter().collect();
        let cur: String = chars[at..at + 1].iter().collect();
        let after: String = chars[at + 1..].iter().collect();
        if !before.is_empty() {
            out.push(Span::styled(before, span.style));
        }
        out.push(Span::styled(
            cur,
            span.style.patch(
                Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
            ),
        ));
        if !after.is_empty() {
            out.push(Span::styled(after, span.style));
        }
        done = true;
    }
    if !done {
        // EOL or empty line: draw the block as a reversed space.
        out.push(Span::styled(
            " ",
            Style::default().add_modifier(ratatui::style::Modifier::REVERSED),
        ));
    }
    Line::from(out)
}

/// `[label]: url` definition (up to 3 leading spaces). Returns normalized key.
pub(crate) fn parse_refdef(line: &str) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i] == ' ' && i < 3 {
        i += 1;
    }
    if chars.get(i) != Some(&'[') {
        return None;
    }
    let mut j = i + 1;
    while j < chars.len() && chars[j] != ']' && chars[j] != '\n' {
        j += 1;
    }
    if j >= chars.len() || chars.get(j + 1) != Some(&':') {
        return None;
    }
    // Require a destination after the colon.
    if chars[j + 2..].iter().all(|c| c.is_whitespace()) {
        return None;
    }
    let label: String = chars[i + 1..j].iter().collect();
    let key = normalize_ref(&label);
    if key.is_empty() {
        return None;
    }
    Some(key)
}

/// Case-insensitive, whitespace-collapsed reference key.
fn normalize_ref(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Opening fence: up to 3 spaces then 3+ backticks/tildes. Returns the char.
fn fence_open(line: &str) -> Option<char> {
    fence_run(line).map(|(c, _)| c)
}

/// Closing fence: up to 3 spaces, 3+ of the same char, then only whitespace.
fn fence_close(line: &str, fence: char) -> bool {
    match fence_run(line) {
        Some((c, rest_ws)) => c == fence && rest_ws,
        None => false,
    }
}

fn fence_run(line: &str) -> Option<(char, bool)> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    let mut spaces = 0;
    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        if chars[i] == ' ' {
            spaces += 1;
        } else {
            spaces += 4;
        }
        i += 1;
    }
    if spaces >= 4 || i >= chars.len() || (chars[i] != '`' && chars[i] != '~') {
        return None;
    }
    let c = chars[i];
    let mut j = i;
    while j < chars.len() && chars[j] == c {
        j += 1;
    }
    if j - i < 3 {
        return None;
    }
    Some((c, chars[j..].iter().all(|x| x.is_whitespace())))
}

/// Fence delimiter line: ticks highlighted, info string dimmed.
fn style_fence_line(line: &str) -> Line<'static> {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() && (chars[i] == ' ' || chars[i] == '\t') {
        i += 1;
    }
    let mut j = i;
    while j < chars.len() && (chars[j] == '`' || chars[j] == '~') {
        j += 1;
    }
    let mut spans = Vec::new();
    if i > 0 {
        spans.push(Span::styled(
            chars[..i].iter().collect::<String>(),
            Style::default(),
        ));
    }
    spans.push(Span::styled(
        chars[i..j].iter().collect::<String>(),
        theme::md_marker(),
    ));
    if j < chars.len() {
        spans.push(Span::styled(
            chars[j..].iter().collect::<String>(),
            theme::md_sym(),
        ));
    }
    Line::from(spans)
}

fn body_base() -> Style {
    theme::base()
}

fn heading_base() -> Style {
    theme::bold()
}

fn link_base(base: Style) -> Style {
    base.patch(theme::md_link_text())
}

fn style_block(line: &str, refs: &HashSet<String>) -> Vec<Span<'static>> {
    let chars: Vec<char> = line.chars().collect();

    // Heading: 1-6 '#' then space/EOL.
    let mut hashes = 0;
    while hashes < chars.len() && hashes < 6 && chars[hashes] == '#' {
        hashes += 1;
    }
    if hashes > 0 && (hashes == chars.len() || chars[hashes] == ' ') {
        let mut out = vec![Span::styled("#".repeat(hashes), theme::md_marker())];
        let mut rest = &chars[hashes..];
        if rest.first() == Some(&' ') {
            out.push(Span::styled(" ", body_base()));
            rest = &rest[1..];
        }
        out.extend(parse_inline(rest, heading_base(), refs));
        return out;
    }

    // Quote: optional indent + '>'. Recurse so `> # H` / `> - x` work.
    let mut qi = 0;
    while qi < chars.len() && (chars[qi] == ' ' || chars[qi] == '\t') {
        qi += 1;
    }
    if qi < chars.len() && chars[qi] == '>' {
        let mut out = Vec::new();
        if qi > 0 {
            out.push(Span::styled(chars[..qi].iter().collect::<String>(), body_base()));
        }
        out.push(Span::styled(">", theme::md_marker()));
        let rest = &chars[qi + 1..];
        let rest_str: String = rest.iter().collect();
        // Keep one optional space after '>' plain, then re-enter block mode.
        let trimmed = rest_str.strip_prefix(' ').unwrap_or(&rest_str);
        if rest_str.len() != trimmed.len() {
            out.push(Span::styled(" ", body_base()));
        }
        out.extend(style_block(trimmed, refs));
        return out;
    }

    // List: indent + (-|*|+|N.|N)) + space/EOL.
    let mut li = 0;
    while li < chars.len() && (chars[li] == ' ' || chars[li] == '\t') {
        li += 1;
    }
    let mut marker_end: Option<usize> = None;
    if li < chars.len() && matches!(chars[li], '-' | '*' | '+')
        && (li + 1 == chars.len() || chars[li + 1] == ' ' || chars[li + 1] == '\t')
    {
        marker_end = Some(li + 1);
    } else {
        let mut j = li;
        while j < chars.len() && chars[j].is_ascii_digit() {
            j += 1;
        }
        if j > li
            && j < chars.len()
            && matches!(chars[j], '.' | ')')
            && (j + 1 == chars.len() || chars[j + 1] == ' ' || chars[j + 1] == '\t')
        {
            marker_end = Some(j + 1);
        }
    }
    if let Some(end) = marker_end {
        let mut out = Vec::new();
        if li > 0 {
            out.push(Span::styled(chars[..li].iter().collect::<String>(), body_base()));
        }
        out.push(Span::styled(
            chars[li..end].iter().collect::<String>(),
            theme::md_marker(),
        ));
        out.extend(parse_inline(&chars[end..], body_base(), refs));
        return out;
    }

    parse_inline(&chars, body_base(), refs)
}

/// Delimiter kinds, longest first.
fn match_opener(chars: &[char], i: usize) -> Option<(Opener, usize)> {
    if starts_with(chars, i, "***") {
        return Some((Opener::BoldItalic, 3));
    }
    if starts_with(chars, i, "**") {
        return Some((Opener::Bold, 2));
    }
    if starts_with(chars, i, "__") {
        return Some((Opener::Underline, 2));
    }
    if chars[i] == '`' {
        let mut len = 0;
        while i + len < chars.len() && chars[i + len] == '`' {
            len += 1;
        }
        return Some((Opener::Code(len), len));
    }
    if chars[i] == '!' && chars.get(i + 1) == Some(&'[') {
        return Some((Opener::Link { bang: true }, 2));
    }
    if chars[i] == '[' {
        return Some((Opener::Link { bang: false }, 1));
    }
    if chars[i] == '*' && next_is(chars, i, |c| !c.is_whitespace()) {
        return Some((Opener::ItalicStar, 1));
    }
    if chars[i] == '_'
        && prev_is(chars, i, |c| !c.is_alphanumeric())
        && next_is(chars, i, |c| !c.is_whitespace())
    {
        return Some((Opener::ItalicUnder, 1));
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Opener {
    BoldItalic,
    Bold,
    Underline,
    ItalicStar,
    ItalicUnder,
    Code(usize),
    Link { bang: bool },
}

fn starts_with(chars: &[char], i: usize, pat: &str) -> bool {
    pat.chars()
        .enumerate()
        .all(|(k, c)| chars.get(i + k) == Some(&c))
}

fn next_is(chars: &[char], i: usize, f: impl Fn(char) -> bool) -> bool {
    chars.get(i + 1).map(|&c| f(c)).unwrap_or(false)
}

fn prev_is(chars: &[char], i: usize, f: impl Fn(char) -> bool) -> bool {
    if i == 0 {
        return true;
    }
    f(chars[i - 1])
}

/// Find the matching closer for `op` at/after `from`. Returns char index.
fn find_closer(chars: &[char], op: Opener, from: usize) -> Option<usize> {
    match op {
        Opener::BoldItalic => find_run(chars, from, "***"),
        Opener::Bold => find_run(chars, from, "**"),
        Opener::Underline => find_run(chars, from, "__"),
        Opener::ItalicStar => {
            let mut j = from;
            while j < chars.len() {
                if chars[j] == '*'
                    && !starts_with(chars, j, "**")
                    && (j == 0 || !chars[j - 1].is_whitespace())
                {
                    // avoid consuming the first star of a "**" closer pair
                    return Some(j);
                }
                j += 1;
            }
            None
        }
        Opener::ItalicUnder => {
            let mut j = from;
            while j < chars.len() {
                if chars[j] == '_'
                    && !starts_with(chars, j, "__")
                    && (j == 0 || !chars[j - 1].is_whitespace())
                    && chars.get(j + 1).map(|&c| !c.is_alphanumeric()).unwrap_or(true)
                {
                    return Some(j);
                }
                j += 1;
            }
            None
        }
        Opener::Code(len) => find_tick_run(chars, from, len),
        Opener::Link { .. } => None, // handled structurally
    }
}

fn find_run(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    let n = pat.chars().count();
    let mut j = from;
    while j + n <= chars.len() {
        if starts_with(chars, j, pat) {
            return Some(j);
        }
        j += 1;
    }
    None
}

/// Closer for code spans: a backtick run of exactly `len`.
fn find_tick_run(chars: &[char], from: usize, len: usize) -> Option<usize> {
    let mut j = from;
    while j < chars.len() {
        if chars[j] == '`' {
            let mut k = j;
            while k < chars.len() && chars[k] == '`' {
                k += 1;
            }
            if k - j == len {
                return Some(j);
            }
            j = k;
        } else {
            j += 1;
        }
    }
    None
}

fn content_style(op: Opener, base: Style) -> Style {
    let add = |m: Modifier| Style::default().add_modifier(m);
    match op {
        Opener::BoldItalic => base.patch(add(Modifier::BOLD | Modifier::ITALIC)),
        Opener::Bold => base.patch(add(Modifier::BOLD)),
        Opener::Underline => base.patch(add(Modifier::UNDERLINED)),
        Opener::ItalicStar | Opener::ItalicUnder => base.patch(add(Modifier::ITALIC)),
        Opener::Code(_) => theme::md_code(),
        Opener::Link { .. } => link_base(base),
    }
}

fn opener_len(op: Opener) -> usize {
    match op {
        Opener::BoldItalic => 3,
        Opener::Bold | Opener::Underline => 2,
        Opener::ItalicStar | Opener::ItalicUnder => 1,
        Opener::Code(len) => len,
        Opener::Link { bang } => {
            if bang {
                2
            } else {
                1
            }
        }
    }
}

fn closer_len(op: Opener) -> usize {
    match op {
        Opener::Code(len) => len,
        _ => opener_len(op),
    }
}

fn delim_text(chars: &[char], at: usize, len: usize) -> String {
    chars[at..at + len].iter().collect()
}

fn parse_inline(chars: &[char], base: Style, refs: &HashSet<String>) -> Vec<Span<'static>> {
    // Fast path: no marker/escape chars means the whole line is one
    // literal span. Covers the vast majority of prose lines.
    if !chars.iter().any(|c| matches!(c, '\\' | '*' | '_' | '`' | '[' | '!')) {
        if chars.is_empty() {
            return Vec::new();
        }
        return vec![Span::styled(chars.iter().collect::<String>(), base)];
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut lit = String::new();
    let mut i = 0;

    macro_rules! flush {
        () => {
            if !lit.is_empty() {
                out.push(Span::styled(std::mem::take(&mut lit), base));
            }
        };
    }

    while i < chars.len() {
        let c = chars[i];
        // Escape: backslash + next char renders literally.
        if c == '\\' && i + 1 < chars.len() {
            lit.push(chars[i + 1]);
            i += 2;
            continue;
        }

        // Links/images are structural: [text](url), ![alt](src).
        if c == '[' || (c == '!' && chars.get(i + 1) == Some(&'[')) {
            if let Some((spans, next)) = try_link(chars, i, base, refs) {
                flush!();
                out.extend(spans);
                i = next;
                continue;
            }
            lit.push(c);
            i += 1;
            continue;
        }

        let Some((op, olen)) = match_opener(chars, i) else {
            lit.push(c);
            i += 1;
            continue;
        };
        // Links handled above; match_opener only reaches here for the rest.
        if matches!(op, Opener::Link { .. }) {
            lit.push(c);
            i += 1;
            continue;
        }
        let content_start = i + olen;
        let Some(close_at) = find_closer(chars, op, content_start) else {
            // Unclosed: render the opener literally.
            lit.push_str(&delim_text(chars, i, olen));
            i += olen;
            continue;
        };
        if close_at == content_start {
            // Empty: `****` etc. render literally.
            lit.push_str(&delim_text(chars, i, olen));
            i += olen;
            continue;
        }
        let inner: String = chars[content_start..close_at].iter().collect();
        // Code content is literal; everything else recurses (nesting).
        let inner_spans = match op {
            Opener::Code(_) => {
                vec![Span::styled(strip_code_spaces(&inner), theme::md_code())]
            }
            _ => parse_inline(&chars[content_start..close_at], content_style(op, base), refs),
        };
        flush!();
        out.push(Span::styled(
            delim_text(chars, i, olen),
            theme::md_sym(),
        ));
        out.extend(inner_spans);
        out.push(Span::styled(
            delim_text(chars, close_at, closer_len(op)),
            theme::md_sym(),
        ));
        let _ = inner;
        i = close_at + closer_len(op);
    }
    flush!();
    out
}

/// CommonMark: one surrounding space inside code spans is stripped.
fn strip_code_spaces(s: &str) -> String {
    if s.len() >= 4 && s.starts_with(' ') && s.ends_with(' ') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Try `[text](url)` / `![alt](src)` at `i`, then `[text][ref]` / `[text][]` /
/// `[text]` when the reference is defined in this document.
/// Returns spans + next index.
fn try_link(
    chars: &[char],
    i: usize,
    base: Style,
    refs: &HashSet<String>,
) -> Option<(Vec<Span<'static>>, usize)> {
    let bang = chars[i] == '!';
    let tstart = if bang { i + 2 } else { i + 1 };
    // First ']' ends the text (no nested-bracket support in v1).
    let mut j = tstart;
    while j < chars.len() && chars[j] != ']' {
        j += 1;
    }
    if j >= chars.len() || tstart == j {
        return None;
    }
    let text: Vec<char> = chars[tstart..j].to_vec();

    // Shared head: [!? [ text(link-styled, nested inline parsed)
    let mut head = Vec::new();
    if bang {
        head.push(Span::styled("!", theme::md_sym()));
    }
    head.push(Span::styled("[", theme::md_sym()));
    head.extend(parse_inline(&text, link_base(base), refs));

    // Inline: [text](url)
    if chars.get(j + 1) == Some(&'(') {
        let mut k = j + 2;
        while k < chars.len() && chars[k] != ')' {
            k += 1;
        }
        if k >= chars.len() {
            return None;
        }
        let url: String = chars[j + 2..k].iter().collect();
        let mut spans = head;
        spans.push(Span::styled("](", theme::md_sym()));
        spans.push(Span::styled(url, theme::md_sym()));
        spans.push(Span::styled(")", theme::md_sym()));
        return Some((spans, k + 1));
    }

    // Reference: [text][ref], collapsed [text][] (= ref text).
    if chars.get(j + 1) == Some(&'[') {
        let mut k = j + 2;
        while k < chars.len() && chars[k] != ']' {
            k += 1;
        }
        if k >= chars.len() {
            return None;
        }
        let raw_ref: String = chars[j + 2..k].iter().collect();
        let key = if raw_ref.trim().is_empty() {
            normalize_ref(&text.iter().collect::<String>())
        } else {
            normalize_ref(&raw_ref)
        };
        if !refs.contains(&key) {
            return None;
        }
        let mut spans = head;
        spans.push(Span::styled("]", theme::md_sym()));
        spans.push(Span::styled("[", theme::md_sym()));
        spans.push(Span::styled(raw_ref, theme::md_sym()));
        spans.push(Span::styled("]", theme::md_sym()));
        return Some((spans, k + 1));
    }

    // Shortcut: [text] when `text` names a defined reference.
    let key = normalize_ref(&text.iter().collect::<String>());
    if !refs.contains(&key) {
        return None;
    }
    let mut spans = head;
    spans.push(Span::styled("]", theme::md_sym()));
    Some((spans, j + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.clone()).collect()
    }

    #[test]
    fn bold_dim_delims() {
        let line = style_line("a **hi** b");
        assert_eq!(cell_text(&line), "a **hi** b");
        let b = &line.spans[1];
        assert_eq!(b.content, "**");
        assert_eq!(b.style, theme::md_sym());
        let t = &line.spans[2];
        assert_eq!(t.content, "hi");
        assert!(t.style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn italic_and_underline() {
        let line = style_line("*it* and __un__");
        assert!(line.spans.iter().any(|s| s.content == "it"
            && s.style.add_modifier.contains(Modifier::ITALIC)));
        assert!(line.spans.iter().any(|s| s.content == "un"
            && s.style.add_modifier.contains(Modifier::UNDERLINED)));
    }

    #[test]
    fn snake_case_underscores_stay_literal() {
        let line = style_line("my_variable_name");
        assert_eq!(line.spans.len(), 1);
        assert_eq!(cell_text(&line), "my_variable_name");
    }

    #[test]
    fn code_span() {
        use ratatui::style::Modifier;
        let line = style_line("run `cargo test` now");
        assert!(line.spans.iter().any(|s| s.content == "cargo test"
            && s.style.add_modifier.contains(Modifier::REVERSED)));
        assert!(line.spans.iter().any(|s| s.content == "`"
            && s.style == theme::md_sym()));
    }

    #[test]
    fn link_parts() {
        let line = style_line("see [docs](https://x.y) ok");
        assert_eq!(cell_text(&line), "see [docs](https://x.y) ok");
        let t = line.spans.iter().find(|s| s.content == "docs").unwrap();
        assert!(t.style.add_modifier.contains(Modifier::UNDERLINED));
        assert!(line.spans.iter().any(|s| s.content == "https://x.y"
            && s.style == theme::md_sym()));
    }

    #[test]
    fn heading_bold_and_marker() {
        let line = style_line("# Title **b**");
        assert_eq!(line.spans[0].content, "#");
        assert_eq!(line.spans[0].style, theme::md_marker());
        assert!(line.spans.iter().any(|s| s.content == "Title "
            && s.style.add_modifier.contains(Modifier::BOLD)));
    }

    #[test]
    fn list_marker_highlighted() {
        for raw in ["- a", "1. b", "2) c", "  * d"] {
            let line = style_line(raw);
            assert!(
                line.spans.iter().any(|s| s.style == theme::md_marker()),
                "{raw}"
            );
            assert_eq!(cell_text(&line), raw);
        }
    }

    #[test]
    fn unclosed_and_escapes_literal() {
        let line = style_line("a **oops");
        assert_eq!(line.spans.len(), 1);
        let line = style_line("a \\*x\\* b");
        assert_eq!(cell_text(&line), "a *x* b");
    }

    #[test]
    fn nested_emphasis() {
        let line = style_line("**bold _both_**");
        let t = line.spans.iter().find(|s| s.content == "both").unwrap();
        assert!(t.style.add_modifier.contains(Modifier::BOLD));
        assert!(t.style.add_modifier.contains(Modifier::ITALIC));
    }

    fn doc_text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.clone()).collect())
            .collect()
    }

    #[test]
    fn fenced_block() {
        let doc = style_doc("```rust\nlet x = 1;\n```\nafter");
        let text = doc_text(&doc);
        assert_eq!(text[1], "let x = 1;");
        assert_eq!(text[3], "after");
        // fence ticks highlighted, info string dimmed
        assert!(doc[0].spans.iter().any(|s| s.content == "```"
            && s.style == theme::md_marker()));
        assert!(doc[0].spans.iter().any(|s| s.content == "rust"
            && s.style == theme::md_sym()));
        // content styled as code
        assert_eq!(doc[1].spans.len(), 1);
        assert_eq!(doc[1].spans[0].style, theme::md_code());
        // unclosed fence swallows the rest
        let doc = style_doc("~~~\ncode");
        assert_eq!(doc[1].spans[0].style, theme::md_code());
    }

    #[test]
    fn reference_links() {
        let doc = style_doc("[a][b]\n[b]: /url\n[c][]\n[c]: /u2\n[s]\n[s]: /u3\n[x][nope]");
        let text = doc_text(&doc);
        assert_eq!(text[0], "[a][b]");
        let t = doc[0].spans.iter().find(|s| s.content == "a").unwrap();
        assert!(t.style.add_modifier.contains(Modifier::UNDERLINED));
        assert_eq!(text[2], "[c][]");
        assert_eq!(text[4], "[s]");
        assert!(doc[4].spans.iter().any(|s| s.content == "s"
            && s.style.add_modifier.contains(Modifier::UNDERLINED)));
        // undefined ref stays a single literal span
        assert_eq!(doc[6].spans.len(), 1);
        // definition lines are dimmed metadata
        assert!(doc[1].spans.iter().all(|s| s.style == theme::md_sym()));
    }

    fn reversed_spans(line: &Line) -> Vec<(String, bool)> {
        line.spans
            .iter()
            .map(|s| {
                (
                    s.content.to_string(),
                    s.style.add_modifier.contains(Modifier::REVERSED),
                )
            })
            .collect()
    }

    #[test]
    fn cursor_reverses_char_only() {
        let line = Line::from(vec![Span::styled("hello", body_base())]);
        let out = with_cursor(line, 1);
        assert_eq!(
            reversed_spans(&out),
            vec![
                ("h".to_string(), false),
                ("e".to_string(), true),
                ("llo".to_string(), false),
            ]
        );
        // full text preserved
        let text: String = out.spans.iter().map(|s| s.content.clone()).collect();
        assert_eq!(text, "hello");
    }

    #[test]
    fn cursor_at_eol_draws_block() {
        let line = Line::from(vec![Span::styled("hi", body_base())]);
        let out = with_cursor(line, 2);
        let spans = reversed_spans(&out);
        assert_eq!(spans.last(), Some(&(" ".to_string(), true)));
        let text: String = out.spans.iter().map(|s| s.content.clone()).collect();
        assert_eq!(text, "hi ");
    }

    #[test]
    fn cursor_on_empty_line_draws_block() {
        let out = with_cursor(Line::from(""), 0);
        assert_eq!(out.spans.len(), 1);
        assert_eq!(out.spans[0].content, " ");
        assert!(out.spans[0]
            .style
            .add_modifier
            .contains(Modifier::REVERSED));
    }

    #[test]
    fn cursor_keeps_surrounding_styles() {
        // bold content keeps BOLD under the reversed cursor
        let line = style_line("**bold**");
        // spans: "**" dim, "bold" bold, "**" dim; cursor on 'o' (col 3)
        let out = with_cursor(line, 3);
        let t = out
            .spans
            .iter()
            .find(|s| s.content == "o")
            .expect("cursor char");
        assert!(t.style.add_modifier.contains(Modifier::REVERSED));
        assert!(t.style.add_modifier.contains(Modifier::BOLD));
    }
}
