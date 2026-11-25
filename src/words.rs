pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '\'' || c == '\u{2019}' || c == '-'
}

/// Match starts (char offsets into `haystack`) for a case-insensitive
/// substring search, in document order. Lowercasing can change char counts
/// (e.g. Turkish dotted I), so lowered chars are mapped back to the source
/// char index they came from.
pub fn find_matches_ci(haystack: &str, needle: &str) -> Vec<usize> {
    let query: Vec<char> = needle.chars().flat_map(|c| c.to_lowercase()).collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut lower: Vec<char> = Vec::new();
    let mut source: Vec<usize> = Vec::new();
    for (i, c) in haystack.chars().enumerate() {
        for lc in c.to_lowercase() {
            lower.push(lc);
            source.push(i);
        }
    }
    let mut out = Vec::new();
    if query.len() > lower.len() {
        return out;
    }
    let mut i = 0;
    while i + query.len() <= lower.len() {
        if lower[i..i + query.len()] == query[..] {
            out.push(source[i]);
            i += query.len(); // non-overlapping, like str::find
        } else {
            i += 1;
        }
    }
    out.dedup(); // expansions can yield the same start twice
    out
}

pub fn count_words(text: &str) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for c in text.chars() {
        if is_word_char(c) {
            if !in_word {
                in_word = true;
                count += 1;
            }
        } else {
            in_word = false;
        }
    }
    count
}

pub fn count_words_rope(rope: &ropey::Rope) -> usize {
    let mut count = 0;
    let mut in_word = false;
    for chunk in rope.chunks() {
        for c in chunk.chars() {
            if is_word_char(c) {
                if !in_word {
                    in_word = true;
                    count += 1;
                }
            } else {
                in_word = false;
            }
        }
    }
    count
}

/// Build the 2-line dim preview shown in the Sheets pane.
pub fn sheet_preview(full_text: &str) -> String {
    preview_from_first_lines(full_text.lines())
}

/// Prefix variant: identical output without copying a whole buffer.
/// Reads only the first ~4KB — previews only ever use the opening lines.
pub fn sheet_preview_rope(rope: &ropey::Rope) -> String {
    const PREFIX_CHARS: usize = 4096;
    let mut buf = String::new();
    'outer: for chunk in rope.chunks() {
        for c in chunk.chars() {
            buf.push(c);
            if buf.len() >= PREFIX_CHARS {
                break 'outer;
            }
        }
    }
    preview_from_first_lines(buf.lines())
}

fn preview_from_first_lines<'a>(lines: impl Iterator<Item = &'a str>) -> String {
    let mut kept: Vec<String> = Vec::new();
    for raw in lines {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        // skip the title line if it looks like a heading, but keep body
        let stripped = t
            .trim_start_matches('#')
            .trim_start_matches('>')
            .trim_start_matches(&['-', '*', '+'][..])
            .trim();
        if stripped.is_empty() {
            continue;
        }
        kept.push(stripped.to_string());
        if kept.len() >= 3 {
            break;
        }
    }
    if kept.is_empty() {
        return "preview of the\ntext...".to_string();
    }
    // Drop a leading title if there are more lines after it
    let body: Vec<String> = if kept.len() > 1 { kept[1..].to_vec() } else { kept };
    let joined = body.join(" ");
    // wrap-ish: keep short, UI will wrap
    const MAX: usize = 90;
    if joined.chars().count() > MAX {
        let s: String = joined.chars().take(MAX).collect();
        format!("{s}...")
    } else {
        // mimic image two-line "preview of the / text..."
        joined
    }
}

#[cfg(test)]
mod find_tests {
    use super::find_matches_ci;

    #[test]
    fn case_insensitive_ascii() {
        assert_eq!(find_matches_ci("foo BAR foo", "foo"), vec![0, 8]);
        assert_eq!(find_matches_ci("foo BAR foo", "BAR"), vec![4]);
        assert_eq!(find_matches_ci("abc", "ABC"), vec![0]);
    }

    #[test]
    fn char_offsets_not_byte_offsets() {
        // 'é' and 'CJK' are multi-byte / wide: results are char indices.
        assert_eq!(find_matches_ci("café x", "é"), vec![3]);
        assert_eq!(find_matches_ci("ab混cd", "混"), vec![2]);
        assert_eq!(find_matches_ci("ab混cd", "CD"), vec![3]);
    }

    #[test]
    fn empty_query_or_no_match() {
        assert!(find_matches_ci("abc", "").is_empty());
        assert!(find_matches_ci("", "a").is_empty());
        assert!(find_matches_ci("abc", "z").is_empty());
        assert!(find_matches_ci("ab", "abcd").is_empty());
    }

    #[test]
    fn non_overlapping_like_str_find() {
        assert_eq!(find_matches_ci("aaa", "aa"), vec![0]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_basic() {
        assert_eq!(count_words("hello world"), 2);
        assert_eq!(count_words("# Sheet 3"), 2);
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words("don't stop"), 2);
    }

    #[test]
    fn deleted_words_still_counted_by_caller() {
        // gross_typed is event-based; this just pins tokenizer stability
        assert_eq!(count_words("hello "), 1);
    }
}

#[cfg(test)]
mod ui_smoke {
    use crate::app::{App, MenuKind};

    #[test]
    fn renders_top_menus_help_and_prompt() {
        let dir = std::env::temp_dir().join(format!("alcott-ui-{}", std::process::id()));
        let lib = dir.join("Library");
        let data = dir.join("data");
        let _ = std::fs::remove_dir_all(&dir);
        let mut app = App::new(lib, data.clone());
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();

        // base + each menu + help + new-sheet prompt must render without panic
        term.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        for kind in [MenuKind::File, MenuKind::Edit, MenuKind::View, MenuKind::Export] {
            app.toggle_menu(kind);
            term.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        }
        app.close_menu();
        app.show_help = true;
        term.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        app.show_help = false;
        app.new_prompt = Some(crate::app::NewSheetPrompt {
            input: "Test".into(),
        });
        term.draw(|f| crate::ui::render(f, &mut app)).unwrap();

        // accelerator letters are highlighted: top row contains F ile / E dit / V iew
        let buf = term.backend().buffer().clone();
        let top: String = (0..120)
            .map(|x| buf[(x, 0)].symbol().to_string())
            .collect();
        assert!(top.contains('F') && top.contains("ile"), "top bar: {top}");
        assert!(top.contains('S') && top.contains("save"), "hints: {top}");

        // top-bar File/Edit/View/Xport hitboxes are recorded for mouse clicks
        assert_eq!(app.menu_bar.len(), 4);
        assert!(app.menu_bar.iter().any(|(k, r)| *k == crate::app::MenuKind::File
            && r.width == 4
            && r.y == 0));

        // panels toggle: hiding both still renders, editor centered ≤80 wide
        app.toggle_panels();
        assert!(!app.show_panels);
        term.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        // persisted
        let state: serde_json::Value = serde_json::from_slice(
            &std::fs::read(data.join("state.json")).expect("state.json written"),
        )
        .unwrap();
        assert_eq!(state["show_panels"], false);
        app.toggle_panels();
        assert!(app.show_panels);

        // group prompt render
        app.focus = crate::app::Focus::Library;
        app.start_new_group();
        term.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        app.group_prompt = None;

        // markdown guide renders with live-styled examples
        app.show_guide = true;
        term.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        app.show_guide = false;

        let _ = std::fs::remove_dir_all(&dir);
    }
}
