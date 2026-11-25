use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus};
use crate::config::theme;

fn sheet_title(name: &str) -> String {
    name.to_string()
}

pub fn render(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(ratatui::widgets::Block::default(), area);

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    render_top(frame, app, vertical[0]);
    render_main(frame, app, vertical[1]);
    render_bottom(frame, app, vertical[2]);
    render_menu_popup(frame, app);
    render_help_overlay(frame, app);
    render_guide_overlay(frame, app);
    render_new_prompt(frame, app);
    render_group_prompt(frame, app);
    render_search_prompt(frame, app);
    render_export_dialog(frame, app);
    render_export_preview(frame, app);
    render_export_prompt(frame, app);
}

fn menu_label_spans(label: &str, accel: char) -> Vec<Span<'static>> {
    // Split label at first occurrence of accel (case-insensitive) so the
    // accelerator letter can be highlighted. Falls back to plain text.
    let target = accel.to_lowercase().next().unwrap_or(accel);
    if let Some(pos) = label.chars().position(|c| c.to_lowercase().next() == Some(target)) {
        let pre: String = label.chars().take(pos).collect();
        let acc: String = label.chars().skip(pos).take(1).collect();
        let post: String = label.chars().skip(pos + 1).collect();
        let mut out = Vec::new();
        if !pre.is_empty() {
            out.push(Span::styled(pre, Style::default()));
        }
        out.push(Span::styled(acc, theme::accel()));
        if !post.is_empty() {
            out.push(Span::styled(post, Style::default()));
        }
        out
    } else {
        vec![Span::styled(label.to_string(), Style::default())]
    }
}

fn render_top(frame: &mut Frame, app: &mut App, area: Rect) {
    // Left: Alcott + menus with highlighted Ctrl accelerators.
    // Right: global key hints, also with highlighted letters.
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // Record clickable File/Edit/View/Xport hitboxes: "Alcott"(6) + "     CTRL +   "(14),
    // then File(4) + " / "(3) + Edit(4) + " / "(3) + View(4) + " / "(3) + Xport(5).
    app.menu_bar.clear();
    {
        use crate::app::MenuKind;
        let mut x = cols[0].x + 6 + 14;
        let end = cols[0].x.saturating_add(cols[0].width);
        for kind in [MenuKind::File, MenuKind::Edit, MenuKind::View, MenuKind::Export] {
            let w = if matches!(kind, MenuKind::Export) { 5 } else { 4 };
            if x + w <= end && area.height > 0 {
                app.menu_bar.push((
                    kind,
                    Rect {
                        x,
                        y: area.y,
                        width: w,
                        height: 1,
                    },
                ));
            }
            x += w + 3;
        }
    }

    let mut left: Vec<Span> = vec![
        Span::styled("Alcott", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("     CTRL +   ", theme::accel_dim()),
        Span::styled("F", theme::accel()),
        Span::styled("ile", theme::dim()),
        Span::styled(" / ", theme::dim()),
        Span::styled("E", theme::accel()),
        Span::styled("dit", theme::dim()),
        Span::styled(" / ", theme::dim()),
        Span::styled("V", theme::accel()),
        Span::styled("iew", theme::dim()),
        Span::styled(" / ", theme::dim()),
        Span::styled("X", theme::accel()),
        Span::styled("port", theme::dim()),
    ];
    // underline the open menu title
    if let Some(m) = app.open_menu {
        let idx = match m {
            crate::app::MenuKind::File => 3,
            crate::app::MenuKind::Edit => 6,
            crate::app::MenuKind::View => 9,
            crate::app::MenuKind::Export => 11,
        };
        left[idx] = Span::styled(
            left[idx].content.clone(),
            theme::accel().add_modifier(Modifier::REVERSED),
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(left)).style(Style::default()),
        cols[0],
    );

    let right = Line::from(vec![
        Span::styled("^", theme::accel_dim()),
        Span::styled("S", theme::accel()),
        Span::styled(" save  ", theme::dim()),
        Span::styled("^", theme::accel_dim()),
        Span::styled("Q", theme::accel()),
        Span::styled(" quit  ", theme::dim()),
        Span::styled("^", theme::accel_dim()),
        Span::styled("P", theme::accel()),
        Span::styled(" panels", theme::dim()),
    ]);
    frame.render_widget(
        Paragraph::new(right)
            .alignment(ratatui::layout::Alignment::Right)
            .style(Style::default()),
        cols[1],
    );
}

fn render_main(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.height == 0 {
        return;
    }
    // One row of vertical margin shared by all panes so their tops align.
    let padded = Rect {
        x: area.x,
        y: area.y + 1.min(area.height),
        width: area.width,
        height: area.height.saturating_sub(2),
    };
    app.main_area = Some(padded);
    if !app.show_panels {
        app.clear_pane_rects();
        render_editor(frame, app, padded);
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((app.lib_frac * 100.0).round().clamp(0.0, 100.0) as u16),
            Constraint::Percentage((app.sheets_frac * 100.0).round().clamp(0.0, 100.0) as u16),
            Constraint::Min(0),
        ])
        .split(padded);

    render_library(frame, app, cols[0]);
    render_sheets(frame, app, cols[1]);
    render_editor(frame, app, cols[2]);
}

/// Pad a row with spaces in its own style so highlights fill the panel width.
fn pad_row(spans: &mut Vec<Span<'static>>, style: Style, used: usize, inner_w: usize) {
    let pad = inner_w.saturating_sub(used);
    if pad > 0 {
        spans.push(Span::styled(" ".repeat(pad), style));
    }
}

fn render_library(frame: &mut Frame, app: &mut App, area: Rect) {
    app.lib_area = Some(area);
    app.lib_rows.clear();
    // Borders::RIGHT draws the separator; content fills the rest.
    let inner_w = area.width.saturating_sub(1) as usize;
    let vis = app.library.visible_groups();
    let n = vis.len();
    // Clamp a stale cursor (e.g. after a rescan removed groups).
    if n > 0 {
        app.lib_cursor = app.lib_cursor.min(n - 1);
    } else {
        app.lib_cursor = 0;
        app.lib_scroll = 0;
    }
    // Scroll-into-view: header row stays pinned, groups scroll below it.
    let height = area.height as usize;
    let header_rows = 1usize;
    let capacity = height.saturating_sub(header_rows);
    let mut scroll = app.lib_scroll.min(n.saturating_sub(1));
    if scroll >= n {
        scroll = 0;
    }
    if n > 0 && capacity > 0 {
        let cursor = app.lib_cursor.min(n - 1);
        if cursor < scroll {
            scroll = cursor;
        } else if cursor >= scroll + capacity {
            scroll = cursor + 1 - capacity;
        }
        // Backfill when the tail leaves empty space (short list / resize).
        let max_scroll = n.saturating_sub(capacity.min(n));
        scroll = scroll.min(max_scroll);
    } else if n == 0 {
        scroll = 0;
    }
    app.lib_scroll = scroll;
    // Editing breadcrumb: the group(s) holding the open sheet(s) get the
    // same reverse card as a selection while the editor has focus.
    let crumbs = app.open_groups();
    let mut items: Vec<ListItem> = Vec::with_capacity(vis.len());
    let end = if capacity == 0 { scroll } else { (scroll + capacity).min(n) };
    for (i, g) in vis.iter().enumerate().take(end).skip(scroll) {
        let collapsed = app.library.collapsed.contains(&g.rel);
        let has_kids = app.library.has_children(&g.rel);
        let glyph = if has_kids { if collapsed { "▸" } else { "▾" } } else { " " };
        let indent = "  ".repeat(g.depth);
        let text = format!("{glyph} {indent}{}", g.name);
        let marked = app.selected_groups.contains(&g.rel);
        let selected = (app.focus == Focus::Library && i == app.lib_cursor)
            || (app.focus == Focus::Editor && crumbs.contains(&g.rel));
        let style = if selected {
            theme::selected()
        } else {
            Style::default()
        };
        let mut spans = vec![Span::styled(text.clone(), style)];
        if marked {
            spans.push(Span::styled(
                " *",
                style.patch(Style::default().add_modifier(Modifier::BOLD)),
            ));
        }
        pad_row(
            &mut spans,
            style,
            str_width(&text) + if marked { 2 } else { 0 },
            inner_w,
        );
        items.push(ListItem::new(Line::from(spans)));
        // header occupies row 0 (Borders::RIGHT draws no top edge)
        app.lib_rows.push((area.y + 1 + (i - scroll) as u16, i));
    }

    let mut lines = vec![ListItem::new(Line::from(vec![Span::styled(
        "Library",
        Style::default().add_modifier(Modifier::UNDERLINED),
    )]))];
    lines.extend(items);

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default())
        .style(Style::default());
    let list = List::new(lines).block(block);
    frame.render_widget(list, area);
}

fn sheet_block_h(preview: &str) -> usize {
    1 + preview.lines().take(2).count() + 1
}

fn render_sheets(frame: &mut Frame, app: &mut App, area: Rect) {
    app.sheets_area = Some(area);
    app.sheet_rows.clear();
    let group = app.selected_group();
    let sheets = app.library.sheets_for_group(group.as_deref());
    let n = sheets.len();
    // Clamp a stale cursor (e.g. group switched to one with fewer sheets).
    if n > 0 {
        app.sheet_cursor = app.sheet_cursor.min(n - 1);
    } else {
        app.sheet_cursor = 0;
        app.sheet_scroll = 0;
    }
    let mut items: Vec<ListItem> = Vec::new();
    items.push(ListItem::new(Line::from(vec![Span::styled(
        "Sheets",
        Style::default(),
    )])));
    items.push(ListItem::new(Line::from("")));
    let inner_w = area.width.saturating_sub(1) as usize;
    // Scroll-into-view: header rows stay pinned, sheet blocks scroll below.
    // Each block is title + up to 2 preview lines + 1 gap row.
    let height = area.height as usize;
    let header_rows = 2usize;
    let capacity = height.saturating_sub(header_rows);
    let heights: Vec<usize> = sheets.iter().map(|s| sheet_block_h(&s.preview)).collect();
    let mut scroll = if n == 0 { 0 } else { app.sheet_scroll.min(n - 1) };
    if n > 0 && capacity > 0 {
        let cursor = app.sheet_cursor.min(n - 1);
        if cursor < scroll {
            scroll = cursor;
        } else {
            // Advance until the cursor block fits in the window.
            while scroll <= cursor {
                let used: usize = heights[scroll..=cursor].iter().sum();
                if used <= capacity {
                    break;
                }
                scroll += 1;
            }
            scroll = scroll.min(cursor);
        }
    } else if n == 0 {
        scroll = 0;
    }
    app.sheet_scroll = scroll;
    let mut row: u16 = 2; // rows used so far (header + blank)
    let mut used = 0usize;
    for (i, s) in sheets.iter().enumerate().skip(scroll) {
        let h = heights[i];
        if used > 0 && used + h > capacity {
            break;
        }
        // Always show at least the scroll sheet even if it alone overflows.
        used += h;
        let is_open = app.is_open_sheet(&s.abs);
        let highlighted = (app.focus == Focus::Sheets && i == app.sheet_cursor)
            || (is_open && app.focus == Focus::Editor);
        let style = if highlighted {
            theme::selected()
        } else {
            Style::default()
        };
        let title = sheet_title(&s.name);
        let marked = app.selected_sheets.contains(&s.abs);
        let mut title_spans = Vec::new();
        if marked {
            title_spans.push(Span::styled(
                "* ",
                style.patch(Style::default().add_modifier(Modifier::BOLD)),
            ));
        }
        title_spans.push(Span::styled(
            title.clone(),
            style.add_modifier(Modifier::BOLD),
        ));
        pad_row(
            &mut title_spans,
            style,
            str_width(&title) + if marked { 2 } else { 0 },
            inner_w,
        );
        items.push(ListItem::new(Line::from(title_spans)));
        app.sheet_rows.push((area.y.saturating_add(row), i));
        row += 1;
        // preview lines dim
        for pl in s.preview.lines().take(2) {
            let preview_style = if highlighted {
                // keep preview readable on reverse video
                theme::selected().patch(Style::default().add_modifier(Modifier::DIM))
            } else {
                theme::dim()
            };
            let mut preview_spans = vec![Span::styled(pl.to_string(), preview_style)];
            if highlighted {
                pad_row(&mut preview_spans, preview_style, str_width(pl), inner_w);
            }
            items.push(ListItem::new(Line::from(preview_spans)));
            row += 1;
        }
        if highlighted {
            let mut sep: Vec<Span<'static>> = Vec::new();
            pad_row(&mut sep, style, 0, inner_w);
            items.push(ListItem::new(Line::from(sep)));
        } else {
            items.push(ListItem::new(Line::from("")));
        }
        row += 1;
    }

    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default())
        .style(Style::default());
    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::app::EDITOR_MAX_WIDTH;
    // Center the text column (max width) in the available space.
    // Vertical margin is applied once in render_main so all panes align.
    let w = area.width.min(EDITOR_MAX_WIDTH);
    let x = area.x + area.width.saturating_sub(w) / 2;
    let centered = Rect {
        x,
        y: area.y,
        width: w,
        height: area.height,
    };
    // pad like mock: left margin (inside the centered block)
    let inner = Rect {
        x: centered.x + 2.min(centered.width),
        y: centered.y,
        width: centered.width.saturating_sub(2),
        height: centered.height,
    };
    app.editor_area = Some(area);
    app.editor_inner = Some(inner);

    // Drawn cursor: reverse-video the character under the cursor instead of
    // relying on the terminal's cursor block (which can render dark-on-dark).
    // Skipped while a prompt is open so the real cursor serves the prompt.
    let draw_fake_cursor = app.focus == Focus::Editor
        && app.new_prompt.is_none()
        && app.group_prompt.is_none();
    let place = if app.multiview.is_some() {
        render_editor_multiview(frame, app, inner, draw_fake_cursor)
    } else {
        render_editor_single(frame, app, inner, draw_fake_cursor)
    };

    // Real cursor: hidden while editing (fake cursor above covers it) and
    // in Library/Sheets/menus; visible only in text prompts (which set the
    // cursor position themselves after this).
    if !draw_fake_cursor && app.focus == Focus::Editor {
        if let Some(p) = place {
            let y = inner.y + p.row;
            let x = inner.x + p.col;
            if y < inner.y + inner.height && x < inner.x + inner.width {
                frame.set_cursor_position((x, y));
            }
        }
    }
}

/// Viewport-relative cursor placement for the real (hardware) cursor.
struct CursorPlace {
    row: u16,
    col: u16,
}

/// Keep-mostly-visible scroll policy shared by both editor modes.
/// Returns the (possibly adjusted) scroll offset in visual rows.
fn keep_visible(cursor_vis: usize, scroll: usize, h: usize) -> usize {
    if h == 0 {
        return 0;
    }
    if cursor_vis < scroll {
        cursor_vis
    } else if cursor_vis - scroll >= h {
        cursor_vis + 1 - h
    } else {
        scroll
    }
}

/// Find the visual row containing `(line, col)` (rope-line space).
/// Falls back to the last row of that line (EOL / dropped-whitespace gap).
fn find_cursor_row(rows: &[crate::view::WrappedRow], line: usize, col: usize) -> Option<usize> {
    let mut fallback = None;
    for (i, r) in rows.iter().enumerate() {
        if r.line != line {
            continue;
        }
        if r.char_start <= col && col < r.char_start + r.char_len {
            return Some(i);
        }
        if r.char_start <= col {
            fallback = Some(i);
        }
    }
    fallback
}

/// Plain text of a wrapped row (styles excluded so columns map exactly).
fn row_text(row: &crate::view::WrappedRow) -> String {
    row.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn render_editor_single(
    frame: &mut Frame,
    app: &mut App,
    inner: Rect,
    draw_fake_cursor: bool,
) -> Option<CursorPlace> {
    use crate::{md_style, view};
    let w = inner.width;
    let h = inner.height as usize;
    app.editor_map.clear();
    app.mv_map.clear();
    if w == 0 || h == 0 || app.editor.is_none() {
        view::render_rows(frame, inner, &[Line::from("")]);
        return None;
    }
    // Clamp a stale cursor, then work with disjoint field borrows.
    let (cline, ccol) = {
        let ed = app.editor.as_mut().expect("checked above");
        ed.cursor = ed.cursor.min(ed.rope.len_chars());
        ed.cursor_line_col()
    };
    let n = app.editor.as_ref().expect("checked above").rope.len_lines().max(1);
    let mut anchor = app.editor_anchor.min(cline.min(n - 1));
    let mut scroll = app.editor_scroll as usize;
    if cline.saturating_sub(anchor) > 5000 || scroll > 50000 {
        anchor = cline;
        scroll = 0;
    }
    let rope = &app.editor.as_ref().expect("checked above").rope;
    let defs = md_style::collect_refdefs_rope(rope);
    let mut fence = md_style::fence_state_at(rope, anchor);
    let mut rows: Vec<view::WrappedRow> = Vec::new();
    let mut li = anchor;
    let mut cursor_covered = false;
    let need = scroll + h;
    while li < n {
        let text = view::strip_newline(&rope.line(li).to_string()).to_string();
        let def = md_style::parse_refdef(&text);
        let styled = md_style::style_line_fenced(&text, &defs, def, &mut fence);
        rows.extend(view::wrap_styled(std::slice::from_ref(&styled), &[li], w));
        if li == cline {
            cursor_covered = true;
        }
        if cursor_covered && rows.len() >= need {
            break;
        }
        li += 1;
    }
    if !cursor_covered {
        // Paranoia path (should be unreachable): re-anchor onto the cursor.
        anchor = cline;
        scroll = 0;
        fence = md_style::fence_state_at(rope, anchor);
        rows.clear();
        let text = view::strip_newline(&rope.line(cline).to_string()).to_string();
        let def = md_style::parse_refdef(&text);
        let styled = md_style::style_line_fenced(&text, &defs, def, &mut fence);
        rows.extend(view::wrap_styled(std::slice::from_ref(&styled), &[cline], w));
    }
    if rows.is_empty() {
        view::render_rows(frame, inner, &[Line::from("")]);
        app.editor_anchor = anchor;
        app.editor_scroll = 0;
        return None;
    }
    scroll = scroll.min(rows.len() - 1);
    let cursor_vis = find_cursor_row(&rows, cline, ccol).unwrap_or(rows.len() - 1);
    // Snap back to the cursor only when it moved (keyboard/editing/clicks);
    // wheel scrolling leaves the viewport where the user put it.
    let cursor_id = app.editor.as_ref().expect("checked above").cursor;
    let moved = app.last_cursor_single != Some(cursor_id);
    app.last_cursor_single = Some(cursor_id);
    if moved {
        scroll = keep_visible(cursor_vis, scroll, h);
    }
    // Bound the window: drop leading rows once the scroll offset grows past
    // a few screens (wheel scrolling without cursor motion). The stored
    // (anchor, scroll) advance, but this frame still renders from the
    // already-wrapped rows, so the slice uses the pre-trim offset.
    let disp_start = scroll;
    if h > 0 && scroll > 4 * h {
        let drop = scroll - h;
        let new_anchor_line = rows[drop.min(rows.len() - 1)].line;
        let mut first = drop;
        while first > 0 && rows[first - 1].line == new_anchor_line {
            first -= 1;
        }
        anchor = new_anchor_line;
        scroll -= first;
    }
    let end = (disp_start + h).min(rows.len());
    let window = &rows[disp_start..end];
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(window.len());
    let mut map: Vec<crate::app::EditorMapRow> = Vec::with_capacity(window.len());
    for r in window {
        let text = row_text(r);
        map.push(crate::app::EditorMapRow {
            line: r.line,
            char_start: r.char_start,
            text: text.clone(),
        });
        lines.push(Line::from(r.spans.clone()));
    }
    // Fake cursor on the wrapped row under the real cursor.
    // (`disp_start` indexes the wrapped rows; `scroll` is the rebased offset
    // stored for the next frame after window trimming.)
    let mut place = None;
    if draw_fake_cursor && cursor_vis >= disp_start {
        if let Some(di) = cursor_vis.checked_sub(disp_start) {
            if di < lines.len() {
                let local = ccol.saturating_sub(rows[disp_start + di].char_start);
                let taken = std::mem::replace(&mut lines[di], Line::from(""));
                lines[di] = md_style::with_cursor(taken, local);
                let col_cells = view::prefix_width(&map[di].text, local.min(map[di].text.chars().count()));
                place = Some(CursorPlace {
                    row: di as u16,
                    col: col_cells,
                });
            }
        }
    }
    if !draw_fake_cursor {
        // Prompt mode: position the hardware cursor the same way.
        if cursor_vis >= disp_start {
            if let Some(di) = cursor_vis.checked_sub(disp_start) {
                if di < map.len() {
                    let local = ccol.saturating_sub(rows[disp_start + di].char_start);
                    let col_cells = view::prefix_width(&map[di].text, local.min(map[di].text.chars().count()));
                    place = Some(CursorPlace {
                        row: di as u16,
                        col: col_cells,
                    });
                }
            }
        }
    }
    view::render_rows(frame, inner, &lines);
    app.editor_anchor = anchor;
    app.editor_scroll = scroll.min(u16::MAX as usize) as u16;
    app.editor_map = map;
    place
}

/// Divider text for multi-view segment boundaries.
fn mv_divider_text(app: &App, seg: usize) -> String {
    let path = &app.multiview.as_ref().expect("multiview").segments[seg].path;
    let rel = path
        .strip_prefix(&app.library_root)
        .map(|r| r.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string());
    format!("── {rel} ──")
}

fn render_editor_multiview(
    frame: &mut Frame,
    app: &mut App,
    inner: Rect,
    draw_fake_cursor: bool,
) -> Option<CursorPlace> {
    use crate::{md_style, view};
    let w = inner.width;
    let h = inner.height as usize;
    app.mv_map.clear();
    app.editor_map.clear();
    let total = app.multiview.as_ref().expect("multiview").flat_len();
    if w == 0 || h == 0 || total == 0 {
        view::render_rows(frame, inner, &[Line::from("")]);
        return None;
    }
    // Clamp the cursor into its segment, then work with disjoint borrows.
    let (cseg, cline, ccol) = {
        let mv = app.multiview.as_mut().expect("multiview");
        let nseg = mv.segments.len().max(1);
        let mut cseg = mv.cursor.0.min(nseg - 1);
        if mv.segments.is_empty() {
            cseg = 0;
        }
        let c = mv.cursor.1;
        if let Some(s) = mv.segments.get(cseg) {
            let c = c.min(s.rope.len_chars());
            mv.cursor = (cseg, c);
            let line = s.rope.char_to_line(c);
            (cseg, line, c - s.rope.line_to_char(line))
        } else {
            (0, 0, 0)
        }
    };
    let cursor_flat = app
        .multiview
        .as_ref()
        .expect("multiview")
        .flat_of(cseg, cline);
    let mut anchor = app
        .multiview
        .as_ref()
        .expect("multiview")
        .anchor
        .min(cursor_flat)
        .min(total - 1);
    let mut scroll = app.multiview.as_ref().expect("multiview").scroll as usize;
    if cursor_flat.saturating_sub(anchor) > 5000 || scroll > 50000 {
        anchor = cursor_flat;
        scroll = 0;
    }
    // Style + wrap the window. Fences/refs reset at every segment boundary
    // (matching the old per-segment `style_doc`), so per-segment state is
    // threaded while walking the flattened rows.
    let mut rows: Vec<view::WrappedRow> = Vec::new();
    // (seg, line-in-seg or None for divider, global flat) per wrapped row
    let mut row_ids: Vec<(usize, Option<usize>)> = Vec::new();
    let mut flat = anchor;
    let mut cursor_covered = false;
    let need = scroll + h;
    // per-segment state, rebuilt when the walk enters a segment
    let mut cur_seg = usize::MAX;
    let mut seg_defs = std::collections::HashSet::new();
    let mut seg_fence: Option<char> = None;
    let mut seg_nlines = 0usize;
    while flat < total {
        if cursor_covered && rows.len() >= need {
            break;
        }
        if flat - anchor > 20000 {
            break;
        }
        let Some((seg, line)) = app.multiview.as_ref().expect("multiview").flat_at(flat) else {
            break;
        };
        if seg != cur_seg {
            cur_seg = seg;
            let mv = app.multiview.as_ref().expect("multiview");
            let rope = &mv.segments[seg].rope;
            seg_defs = md_style::collect_refdefs_rope(rope);
            seg_nlines = rope.len_lines().max(1);
            // Fence state at the first line of this segment inside the window.
            let entry: usize = line.unwrap_or_default();
            seg_fence = md_style::fence_state_at(rope, entry.min(rope.len_lines()));
        }
        match line {
            None => {
                // Divider row (never part of any file).
                let text = mv_divider_text(app, seg);
                let styled = Line::from(vec![Span::styled(text, theme::dim())]);
                let before = rows.len();
                rows.extend(view::wrap_styled(std::slice::from_ref(&styled), &[flat], w));
                for _ in before..rows.len() {
                    row_ids.push((seg, None));
                }
                flat += 1;
            }
            Some(l) => {
                let l = l.min(seg_nlines.saturating_sub(1));
                let mv = app.multiview.as_ref().expect("multiview");
                let rope = &mv.segments[seg].rope;
                if l >= rope.len_lines() {
                    flat += 1;
                    continue;
                }
                let text = view::strip_newline(&rope.line(l).to_string()).to_string();
                let def = md_style::parse_refdef(&text);
                let styled = md_style::style_line_fenced(&text, &seg_defs, def, &mut seg_fence);
                let before = rows.len();
                rows.extend(view::wrap_styled(std::slice::from_ref(&styled), &[flat], w));
                for _ in before..rows.len() {
                    row_ids.push((seg, Some(l)));
                }
                if flat == cursor_flat {
                    cursor_covered = true;
                }
                flat += 1;
            }
        }
    }
    if !cursor_covered {
        // Paranoia path: re-anchor onto the cursor and render one segment line.
        anchor = cursor_flat;
        scroll = 0;
        rows.clear();
        row_ids.clear();
        let mv = app.multiview.as_ref().expect("multiview");
        if let Some((seg, Some(line))) = mv.flat_at(cursor_flat) {
            let rope = &mv.segments[seg].rope;
            if line < rope.len_lines() {
                let defs = md_style::collect_refdefs_rope(rope);
                let mut fence = md_style::fence_state_at(rope, line.min(rope.len_lines()));
                let text = view::strip_newline(&rope.line(line).to_string()).to_string();
                let def = md_style::parse_refdef(&text);
                let styled = md_style::style_line_fenced(&text, &defs, def, &mut fence);
                let before = rows.len();
                rows.extend(view::wrap_styled(std::slice::from_ref(&styled), &[cursor_flat], w));
                for _ in before..rows.len() {
                    row_ids.push((seg, Some(line)));
                }
            }
        }
    }
    if rows.is_empty() {
        view::render_rows(frame, inner, &[Line::from("")]);
        if let Some(mv) = app.multiview.as_mut() {
            mv.anchor = anchor;
            mv.scroll = 0;
        }
        return None;
    }
    scroll = scroll.min(rows.len() - 1);
    let cursor_vis = find_cursor_row(&rows, cursor_flat, ccol).unwrap_or(rows.len() - 1);
    // Snap back only when the cursor itself moved (see single-buffer path).
    // Identity is (flat line, column): unique per buffer position.
    let moved = app.last_cursor_mv != Some((cursor_flat, ccol));
    app.last_cursor_mv = Some((cursor_flat, ccol));
    if moved {
        scroll = keep_visible(cursor_vis, scroll, h);
    }
    // Same trim rule as the single-buffer path: advance the stored anchor
    // but keep rendering from the already-wrapped rows.
    let disp_start = scroll;
    if h > 0 && scroll > 4 * h {
        let drop = scroll - h;
        let new_anchor_flat = rows[drop.min(rows.len() - 1)].line;
        let mut first = drop;
        while first > 0 && rows[first - 1].line == new_anchor_flat {
            first -= 1;
        }
        if let Some(mv) = app.multiview.as_mut() {
            mv.anchor = new_anchor_flat;
        }
        scroll -= first;
    } else if let Some(mv) = app.multiview.as_mut() {
        mv.anchor = anchor;
    }
    let scroll_u = scroll.min(u16::MAX as usize);
    if let Some(mv) = app.multiview.as_mut() {
        mv.scroll = scroll_u as u16;
    }
    let end = (disp_start + h).min(rows.len());
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(end.saturating_sub(disp_start));
    let mut map: Vec<crate::app::MvMapRow> = Vec::with_capacity(end.saturating_sub(disp_start));
    for (ri, r) in rows.iter().enumerate().take(end).skip(disp_start) {
        let (seg, line) = row_ids[ri];
        let text: String = r.spans.iter().map(|s| s.content.as_ref()).collect();
        // Dividers resolve to the following segment's start (matches locate).
        let (mseg, mline) = match line {
            Some(l) => (seg, l),
            None => match row_ids.get(ri + 1) {
                Some((ns, Some(nl))) => (*ns, *nl),
                _ => match app.multiview.as_ref().expect("multiview").flat_at(r.line + 1) {
                    Some((ns, Some(nl))) => (ns, nl),
                    _ => (seg, 0),
                },
            },
        };
        // Divider rows carry no text of their own: clicks land on the
        // resolved segment start.
        let (mtext, mstart) = match line {
            Some(_) => (text.clone(), r.char_start),
            None => (String::new(), 0),
        };
        map.push(crate::app::MvMapRow {
            seg: mseg,
            line: mline,
            char_start: if matches!(line, Some(_)) { mstart } else { 0 },
            text: mtext,
        });
        lines.push(Line::from(r.spans.clone()));
    }
    // Fake cursor on the wrapped row under the real cursor (editing), or
    // hardware-cursor placement (prompt mode).
    let mut place = None;
    if cursor_vis >= disp_start {
        if let Some(di) = cursor_vis.checked_sub(disp_start) {
            if di < lines.len() && di < map.len() {
                let local = ccol.saturating_sub(rows[disp_start + di].char_start);
                let capped = local.min(map[di].text.chars().count());
                if draw_fake_cursor {
                    let taken = std::mem::replace(&mut lines[di], Line::from(""));
                    lines[di] = md_style::with_cursor(taken, local);
                } else {
                    let col_cells = view::prefix_width(&map[di].text, capped);
                    place = Some(CursorPlace {
                        row: di as u16,
                        col: col_cells,
                    });
                }
            }
        }
    }
    view::render_rows(frame, inner, &lines);
    app.mv_map = map;
    place
}

/// Width-aware middle truncation: keeps both ends (`My gr…ft v2.md`).
/// Falls back to `…`/empty when almost no room.
fn truncate_middle(s: &str, max_width: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let width: usize = s.chars().filter_map(|c| c.width()).sum();
    if width <= max_width {
        return s.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let budget = max_width - 1; // 1 column for …
    let left_budget = budget.div_ceil(2);
    let right_budget = budget - left_budget;
    let mut left = String::new();
    let mut lw = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if lw + cw > left_budget {
            break;
        }
        left.push(c);
        lw += cw;
    }
    let mut right_chars: Vec<char> = Vec::new();
    let mut rw = 0;
    for c in s.chars().rev() {
        let cw = c.width().unwrap_or(0);
        if rw + cw > right_budget {
            break;
        }
        right_chars.push(c);
        rw += cw;
    }
    right_chars.reverse();
    let right: String = right_chars.into_iter().collect();
    format!("{left}…{right}")
}

fn str_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}

/// Statusbar path cascade: full → rel (no `~/Documents/Alcott/Library`) →
/// file only → middle-truncated title. Returns the first candidate that fits.
fn shorten_path(full: &str, rel: &str, file: &str, max_width: usize) -> String {
    if str_width(full) <= max_width {
        return full.to_string();
    }
    if str_width(rel) <= max_width {
        return rel.to_string();
    }
    if str_width(file) <= max_width {
        return file.to_string();
    }
    truncate_middle(file, max_width)
}

/// Stats cascade, longest first: full → drop `today` → drop typed clause →
/// drop net clause → label only. The caller truncates the label if even it
/// doesn't fit.
fn stats_candidates(label: &str, words: usize, net_str: &str, typed: usize) -> Vec<String> {
    vec![
        format!("{label}  |  {words} words  |  {net_str} net · {typed} typed today"),
        format!("{label}  |  {words} words  |  {net_str} net · {typed} typed"),
        format!("{label}  |  {words} words  |  {net_str} net"),
        format!("{label}  |  {words} words"),
        label.to_string(),
    ]
}

/// Statusbar pane hints: (key, label) pairs. Keys render bold+underlined,
/// matching the top-bar accelerator convention.
const LIB_HINTS_FULL: &[(&str, &str)] = &[
    ("j/k", " move"),
    ("h/l", " expand"),
    ("Enter", " sheets"),
    ("N", " new"),
    ("R", " rename"),
    ("D", " delete"),
    ("X", " mark"),
    ("M", " move"),
];
const LIB_HINTS_MED: &[(&str, &str)] = &[
    ("Enter", " sheets"),
    ("N", " new"),
    ("R", " rename"),
    ("D", " delete"),
    ("X", " mark"),
    ("M", " move"),
];
const SHEET_HINTS_FULL: &[(&str, &str)] = &[
    ("j/k", " move"),
    ("Enter", " open"),
    ("N", " new"),
    ("r", " rename"),
    ("D", " delete"),
    ("X", " mark"),
    ("A", " all"),
    ("J/K", " reorder"),
    ("M", " move"),
];
const SHEET_HINTS_MED: &[(&str, &str)] = &[
    ("Enter", " open"),
    ("N", " new"),
    ("r", " rename"),
    ("D", " delete"),
    ("X", " mark"),
    ("A", " all"),
    ("J/K", " reorder"),
    ("M", " move"),
];
const HINTS_MIN: &[(&str, &str)] = &[("X", " mark"), ("M", " move")];

/// Build hint spans plus the plain text (for width measurement).
fn hint_spans(parts: &[(&str, &str)]) -> (Vec<Span<'static>>, String) {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut plain = String::new();
    for (i, (key, label)) in parts.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", theme::dim()));
            plain.push_str(" · ");
        }
        spans.push(Span::styled((*key).to_string(), theme::accel()));
        spans.push(Span::styled(
            (*label).to_string(),
            theme::dim(),
        ));
        plain.push_str(key);
        plain.push_str(label);
    }
    (spans, plain)
}

/// Richest hint tier that fits: full → actions (+Enter) → minimal.
/// Falls back to a middle-truncated minimal line.
fn hint_tier(
    full: &[(&str, &str)],
    med: &[(&str, &str)],
    budget: usize,
) -> (Vec<Span<'static>>, usize) {
    for parts in [full, med, HINTS_MIN] {
        let (spans, plain) = hint_spans(parts);
        if str_width(&plain) <= budget {
            let w = str_width(&plain);
            return (spans, w);
        }
    }
    let (_, plain) = hint_spans(HINTS_MIN);
    let t = truncate_middle(&plain, budget);
    let w = str_width(&t);
    (
        vec![Span::styled(t, theme::dim())],
        w,
    )
}

fn render_bottom(frame: &mut Frame, app: &App, area: Rect) {
    let total = area.width as usize;
    if total == 0 || area.height == 0 {
        return;
    }
    let full = app.display_path();
    let rel = app.rel_display_path();
    let file = app.file_display_name();
    let net = app.stats.net(app.library.total_words);
    let net_str = if net >= 0 {
        format!("+{net}")
    } else {
        format!("{net}")
    };
    let label = app.status_label();
    let rights = stats_candidates(&label, app.editor_words, &net_str, app.stats.gross_typed);

    // Prefer full stats; shrink the path first, stats only if necessary.
    // left + ≥1 gap + right must fit, unless the path is empty.
    let mut chosen_right = String::new();
    let mut left_max = 0;
    for r in &rights {
        if str_width(r) < total {
            chosen_right = r.clone();
            left_max = total - str_width(r) - 1;
            break;
        }
    }
    if chosen_right.is_empty() {
        // Even the label alone doesn't fit: stats only, truncated.
        chosen_right = truncate_middle(rights.last().cloned().unwrap_or_default().as_str(), total);
        let line = Line::from(vec![Span::styled(chosen_right, theme::dim())]);
        frame.render_widget(
            Paragraph::new(line).style(Style::default()),
            area,
        );
        return;
    }

    // Left side: the editor keeps the path; panels show control hints
    // instead (the path is irrelevant while navigating). Move mode
    // prepends its banner with the live drop target.
    let banner = app.move_banner().unwrap_or_default();
    let bw = str_width(&banner);
    let budget = if banner.is_empty() {
        left_max
    } else {
        left_max.saturating_sub(bw)
    };
    let (mut left_spans, mut lw) = match app.focus {
        Focus::Library => hint_tier(LIB_HINTS_FULL, LIB_HINTS_MED, budget),
        Focus::Sheets => hint_tier(SHEET_HINTS_FULL, SHEET_HINTS_MED, budget),
        Focus::Editor => {
            let s = shorten_path(&full, &rel, &file, budget);
            let w = str_width(&s);
            (
                vec![Span::styled(s, theme::dim())],
                w,
            )
        }
    };
    if !banner.is_empty() {
        if bw + 1 + str_width(&chosen_right) > total {
            // banner itself doesn't fit: truncate it, drop the rest
            let t = truncate_middle(&banner, total.saturating_sub(str_width(&chosen_right) + 1));
            let w = str_width(&t);
            left_spans = vec![Span::styled(t, theme::dim())];
            lw = w;
        } else {
            left_spans.insert(0, Span::styled(banner, theme::dim()));
            lw += bw;
        }
    }
    let gap = total.saturating_sub(lw + str_width(&chosen_right));
    let mut spans = left_spans;
    if gap > 0 {
        spans.push(Span::styled(" ".repeat(gap), theme::dim()));
    }
    spans.push(Span::styled(chosen_right, theme::dim()));
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::default()),
        area,
    );
}

fn popup_rect(area: Rect, w: u16, h: u16, x: u16, y: u16) -> Rect {
    Rect {
        x: area.x + x.min(area.width.saturating_sub(w)),
        y: area.y + y,
        width: w.min(area.width),
        height: h.min(area.height.saturating_sub(y)),
    }
}

fn render_menu_popup(frame: &mut Frame, app: &mut App) {
    let Some(kind) = app.open_menu else {
        app.menu_area = None;
        return;
    };
    let items = app.menu_items();
    let title = kind.title();
    let h = (items.len() + 2) as u16;
    let area = popup_rect(frame.area(), 30, h, 12, 1);
    app.menu_area = Some(area);
    let mut list_items: Vec<ListItem> = Vec::new();
    for (i, (label, accel)) in items.iter().enumerate() {
        let selected = i == app.menu_cursor;
        let mut spans = menu_label_spans(label, *accel);
        // suffix the ctrl hint for mouse-free discovery
        spans.push(Span::styled(
            format!("  ^{}", accel.to_uppercase()),
            theme::dim(),
        ));
        let style = if selected {
            theme::selected().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        list_items.push(ListItem::new(Line::from(spans)).style(style));
    }
    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(List::new(list_items).block(block), area);
}

fn render_help_overlay(frame: &mut Frame, app: &App) {
    if !app.show_help {
        return;
    }
    let area = frame.area();
    let w = 56.min(area.width.saturating_sub(4));
    let lines = vec![
        Line::from(vec![
            Span::styled("F", theme::accel()),
            Span::styled("ile  ", Style::default()),
            Span::styled("E", theme::accel()),
            Span::styled("dit  ", Style::default()),
            Span::styled("V", theme::accel()),
            Span::styled("iew menus (Ctrl+letter)", Style::default()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("1", theme::accel()),
            Span::styled("/2/3 focus panes   Tab cycle", Style::default()),
        ]),
        Line::from(vec![Span::styled(
            "Alt+1/2/3 (or Ctrl+1/2/3) jumps panes from the editor",
            theme::dim(),
        )]),
        Line::from(vec![
            Span::styled("S", theme::accel()),
            Span::styled(" save (Ctrl+S)   ", Style::default()),
            Span::styled("Q", theme::accel()),
            Span::styled(" quit (Ctrl+Q)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("N", theme::accel()),
            Span::styled(" new sheet (File menu)   ", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("K", theme::accel()),
            Span::styled(" delete line (Edit menu)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("D", theme::accel()),
            Span::styled(" delete sheet (File menu)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("Z", theme::accel()),
            Span::styled(" undo · ", Style::default()),
            Span::styled("Y", theme::accel()),
            Span::styled(" redo (Ctrl+Z / Ctrl+Y)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("F", theme::accel()),
            Span::styled(" find in sheet (Edit menu, Ctrl+G)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("Z", theme::accel()),
            Span::styled(" undo · ", Style::default()),
            Span::styled("Y", theme::accel()),
            Span::styled(" redo (Ctrl+Z / Ctrl+Y)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("F", theme::accel()),
            Span::styled(" find in sheet (Edit menu, or Ctrl+G)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("P", theme::accel()),
            Span::styled(" toggle panels (Ctrl+P)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("N", theme::accel()),
            Span::styled("/", Style::default()),
            Span::styled("R", theme::accel()),
            Span::styled("/", Style::default()),
            Span::styled("D", theme::accel()),
            Span::styled(" new/rename/delete group (Library)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("N", theme::accel()),
            Span::styled("/", Style::default()),
            Span::styled("r", theme::accel()),
            Span::styled("/", Style::default()),
            Span::styled("D", theme::accel()),
            Span::styled(" new/rename/delete sheet (Sheets)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("X", theme::accel()),
            Span::styled(" mark · ", Style::default()),
            Span::styled("A", theme::accel()),
            Span::styled(" mark all · ", Style::default()),
            Span::styled("M", theme::accel()),
            Span::styled(" move · ", Style::default()),
            Span::styled("J", theme::accel()),
            Span::styled("/", Style::default()),
            Span::styled("K", theme::accel()),
            Span::styled(" reorder (Sheets)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("M", theme::accel()),
            Span::styled(" markdown guide (View menu)", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("X", theme::accel()),
            Span::styled("port menu (Ctrl+X) · CLI: export", Style::default()),
        ]),
        Line::from(vec![
            Span::styled("O", theme::accel()),
            Span::styled(" quick preview (Ctrl+O, no export menu)", Style::default()),
        ]),
        Line::from(vec![Span::styled(
            "Click select/open · wheel scroll · drag separators · mouse: View menu",
            theme::dim(),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Esc closes · highlighted letter = Ctrl+key",
            theme::dim(),
        )]),
    ];
    // Size to the content (like the Markdown guide below): the next help
    // line added never silently clips.
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + (area.height - h) / 2,
        width: w,
        height: h,
    };
    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

/// View > Markdown guide: syntax table with live-styled examples.
fn render_guide_overlay(frame: &mut Frame, app: &App) {
    if !app.show_guide {
        return;
    }
    use crate::md_style;
    let rows: &[&str] = &[
        "**bold**",
        "*italic*",
        "__underlined__",
        "`code`",
        "[text](url)",
        "[text][ref]",
        "# Heading",
        "- item",
        "1. item",
        "> quote",
        "```rust",
    ];
    // One doc so `[ref]` resolves and the fence row styles as an opener.
    // The closing fence + definition lines are skipped when zipping rows.
    let doc = format!("{}\n```\n[ref]: https://alcott", rows.join("\n"));
    let styled = md_style::style_doc(&doc);
    let mut lines: Vec<Line> = Vec::new();
    for (raw, rendered) in rows.iter().zip(styled.iter()) {
        let mut spans = vec![Span::styled(
            format!("{raw:<14}"),
            theme::dim(),
        )];
        spans.extend(rendered.spans.clone());
        lines.push(Line::from(spans));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Esc closes · sidebar: View > Markdown guide (M)",
        theme::dim(),
    )]));
    let area = frame.area();
    let w = 58.min(area.width.saturating_sub(4));
    let h = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let rect = Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    };
    let block = Block::default()
        .title(" Markdown guide ")
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(Paragraph::new(lines).block(block), rect);
}

fn render_new_prompt(frame: &mut Frame, app: &App) {
    let Some(p) = app.new_prompt.as_ref() else {
        return;
    };
    let area = frame.area();
    let w = 48.min(area.width.saturating_sub(4));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + area.height / 2,
        width: w,
        height: 3,
    };
    let (title, label, cursor_off): (&str, &str, u16) = if app.sheet_rename_target.is_some() {
        (
            " Sheets > Rename (Enter applies, Esc cancels) ",
            "Name: ",
            8,
        )
    } else {
        (
            " File > New (Enter creates, Esc cancels) ",
            "New sheet: ",
            13,
        )
    };
    let line = Line::from(vec![
        Span::styled(label, Style::default()),
        Span::styled(
            p.input.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(Paragraph::new(line).block(block), rect);
    frame.set_cursor_position((
        rect.x + cursor_off + p.input.chars().count() as u16,
        rect.y + 1,
    ));
}

fn render_search_prompt(frame: &mut Frame, app: &App) {
    let Some(p) = app.search_prompt.as_ref() else {
        return;
    };
    let area = frame.area();
    let w = 52.min(area.width.saturating_sub(4));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + area.height / 2,
        width: w,
        height: 3,
    };
    let line = Line::from(vec![
        Span::styled("Find: ", Style::default()),
        Span::styled(
            p.input.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::default()
        .title(" Find in sheet (Enter: next match, Esc: done) ")
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(Paragraph::new(line).block(block), rect);
    // "Find: " label + border.
    frame.set_cursor_position((rect.x + 8 + p.input.chars().count() as u16, rect.y + 1));
}

fn render_group_prompt(frame: &mut Frame, app: &App) {
    let Some(p) = app.group_prompt.as_ref() else {
        return;
    };
    let (title, label) = match &p.kind {
        crate::app::GroupPromptKind::New { parent } => {
            let where_ = if parent.as_os_str().is_empty() {
                "top level".to_string()
            } else {
                parent.display().to_string()
            };
            (
                format!(" Library > New group in {where_} (Enter creates, Esc cancels) "),
                "Name: ",
            )
        }
        crate::app::GroupPromptKind::Rename { .. } => (
            " Library > Rename group (Enter applies, Esc cancels) ".to_string(),
            "Name: ",
        ),
    };
    let area = frame.area();
    let w = 52.min(area.width.saturating_sub(4));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + area.height / 2,
        width: w,
        height: 3,
    };
    let line = Line::from(vec![
        Span::styled(label, Style::default()),
        Span::styled(
            p.input.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(Paragraph::new(line).block(block), rect);
    frame.set_cursor_position((
        rect.x + label.chars().count() as u16 + 1 + p.input.chars().count() as u16,
        rect.y + 1,
    ));
}

fn render_export_dialog(frame: &mut Frame, app: &mut App) {
    if !app.show_export {
        return;
    }
    let area = frame.area();
    let w = 76.min(area.width.saturating_sub(4));
    let h = 22.min(area.height.saturating_sub(4));
    let rect = Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    };
    frame.render_widget(ratatui::widgets::Clear, rect);
    let block = Block::default()
        .title(" Export (CSS drives output · Tab switches column · Enter saves) ")
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    frame.render_widget(block.clone(), rect);
    let inner = Rect {
        x: rect.x + 1,
        y: rect.y + 1,
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    };
    if inner.width < 20 || inner.height < 8 {
        return;
    }
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(inner);
    // Left: styles.
    let styles = app.export_style_names();
    let mut items: Vec<ListItem> = Vec::new();
    items.push(ListItem::new(Line::from(vec![Span::styled(
        format!("Style{}", if app.export_col == 0 { " ◀" } else { "" }),
        Style::default().add_modifier(Modifier::UNDERLINED),
    )])));
    for (i, name) in styles.iter().enumerate() {
        let selected = i == app.export_style_cursor;
        let style = if selected {
            theme::selected().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let marker = if name == &app.export_css_name { "● " } else { "  " };
        items.push(ListItem::new(Line::from(vec![Span::styled(format!("{marker}{name}"), style)])));
    }
    // Include attached-fonts note as trailing rows.
    items.push(ListItem::new(Line::from("")));
    items.push(ListItem::new(Line::from(vec![Span::styled(
        if app.attached_fonts.is_empty() {
            "fonts: none (F attach, C clear)".to_string()
        } else {
            format!("fonts: {} attached (C clear)", app.attached_fonts.len())
        },
        theme::dim(),
    )])));
    frame.render_widget(List::new(items).block(Block::default()), cols[0]);
    // Right: formats + output.
    let order = crate::export::ExportFormat::grid_order();
    let mut ritems: Vec<ListItem> = Vec::new();
    ritems.push(ListItem::new(Line::from(vec![Span::styled(
        format!("Format{}", if app.export_col == 1 { " ◀" } else { "" }),
        Style::default().add_modifier(Modifier::UNDERLINED),
    )])));
    for (i, fmt) in order.iter().enumerate() {
        let selected = i == app.export_format_cursor;
        let style = if selected {
            theme::selected().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let marker = if *fmt == app.export_format { "● " } else { "  " };
        ritems.push(ListItem::new(Line::from(vec![Span::styled(format!("{marker}{}", fmt.label()), style)])));
    }
    ritems.push(ListItem::new(Line::from("")));
    ritems.push(ListItem::new(Line::from(vec![Span::styled(
        format!("out: {}", app.export_output),
        theme::dim(),
    )])));
    frame.render_widget(List::new(ritems).block(Block::default()), cols[1]);
    // Footer hints (bottom row of dialog).
    if inner.height >= 2 {
        let footer = Rect {
            x: inner.x,
            y: inner.y + inner.height - 1,
            width: inner.width,
            height: 1,
        };
        let preview_hint: &str = match app.export_format {
            crate::export::ExportFormat::Pdf | crate::export::ExportFormat::Epub => {
                if cfg!(target_os = "linux") {
                    "P zathura"
                } else {
                    "P preview"
                }
            }
            _ => "P preview",
        };
        let zathura_note: &str = if app.export_zathura_path.is_some()
            && matches!(
                app.export_format,
                crate::export::ExportFormat::Pdf | crate::export::ExportFormat::Epub
            ) {
            " · live in zathura"
        } else {
            ""
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                format!("S save · U safe-save · {preview_hint} · O open · T path · N/E/I/D style · F font · Esc close{zathura_note}"),
                theme::dim(),
            )])),
            footer,
        );
    }
}

fn render_export_preview(frame: &mut Frame, app: &App) {
    if !app.export_preview {
        return;
    }
    // External viewer replaces the overlay for PDF/ePub once launched.
    if app.export_zathura_path.is_some()
        && matches!(
            app.export_format,
            crate::export::ExportFormat::Pdf | crate::export::ExportFormat::Epub
        )
    {
        return;
    }
    let area = frame.area();
    let w = 72.min(area.width.saturating_sub(6));
    let h = 18.min(area.height.saturating_sub(6));
    let rect = Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + 2,
        width: w,
        height: h,
    };
    frame.render_widget(ratatui::widgets::Clear, rect);
    let block = Block::default()
        .title(format!(" Preview · {} · {} ", app.export_format.short(), app.export_css_name))
        .borders(Borders::ALL)
        .border_style(Style::default())
        .style(Style::default());
    let lines: Vec<Line> = app
        .export_preview_text(200)
        .into_iter()
        .map(|s| Line::from(vec![Span::styled(s, Style::default())]))
        .collect();
    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), rect);
}

fn render_export_prompt(frame: &mut Frame, app: &App) {
    let Some(p) = app.export_prompt.as_ref() else {
        return;
    };
    let (title, label) = match p.kind {
        crate::app::ExportPromptKind::OutputPath => (" Export > Output path (Enter applies, Esc cancels) ", "Path: "),
        crate::app::ExportPromptKind::ImportCss => (" Export > Import .css (Enter imports, Esc cancels) ", "CSS file: "),
        crate::app::ExportPromptKind::AttachFont => (" Export > Attach font (Enter attaches, Esc cancels) ", "Font file: "),
        crate::app::ExportPromptKind::NewStyle => (" Export > New style (Enter creates, Esc cancels) ", "Name: "),
    };
    let area = frame.area();
    let w = 64.min(area.width.saturating_sub(4));
    let rect = Rect {
        x: area.x + (area.width - w) / 2,
        y: area.y + area.height / 2 + 8,
        width: w,
        height: 3,
    };
    let line = Line::from(vec![
        Span::styled(label, Style::default()),
        Span::styled(p.input.clone(), Style::default().add_modifier(Modifier::BOLD)),
    ]);
    let block = Block::default().title(title).borders(Borders::ALL);
    frame.render_widget(ratatui::widgets::Clear, rect);
    frame.render_widget(Paragraph::new(line).block(block), rect);
    frame.set_cursor_position((
        rect.x + label.chars().count() as u16 + 1 + p.input.chars().count() as u16,
        rect.y + 1,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bottom_row(term: &ratatui::Terminal<ratatui::backend::TestBackend>, w: u16, h: u16) -> String {
        let buf = term.backend().buffer().clone();
        (0..w)
            .map(|x| buf[(x, h - 1)].symbol().to_string())
            .collect()
    }

    fn hint_app(tag: &str) -> (crate::app::App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("alcott-hint-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let app = crate::app::App::new(dir.join("Library"), dir.join("data"));
        (app, dir)
    }

    #[test]
    fn middle_truncate_keeps_both_ends() {
        assert_eq!(truncate_middle("Sheet3.md", 20), "Sheet3.md");
        assert_eq!(truncate_middle("Sheet3.md", 9), "Sheet3.md");
        let t = truncate_middle("My great novel draft v2.md", 12);
        assert_eq!(str_width(&t), 12);
        assert!(t.contains('…'));
        assert!(t.ends_with(".md"));
        assert_eq!(truncate_middle("abc", 1), "…");
        assert_eq!(truncate_middle("abc", 0), "");
    }

    #[test]
    fn path_cascade_order() {
        let full = "~/Documents/Alcott/Library/Writing/Book\\ 1/Sheet3.md";
        let rel = "Writing/Book\\ 1/Sheet3.md";
        let file = "Sheet3.md";
        assert_eq!(shorten_path(full, rel, file, 200), full);
        assert_eq!(shorten_path(full, rel, file, str_width(rel)), rel);
        assert_eq!(shorten_path(full, rel, file, str_width(file)), file);
        // too narrow for even the file: middle-truncated title
        let t = shorten_path(full, rel, file, 6);
        assert_eq!(str_width(&t), 6);
        assert!(t.contains('…'));
    }

    #[test]
    fn stats_shrink_when_squeezed() {
        let rights = stats_candidates("saved", 1250, "+120", 543);
        assert!(rights[0].contains("typed today"));
        assert!(!rights[1].contains("today"));
        assert!(!rights[3].contains("net"));
        assert_eq!(rights[4], "saved");
    }

    #[test]
    fn narrow_statusbar_keeps_gap() {
        use crate::app::App;
        let dir = std::env::temp_dir().join(format!("alcott-sb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut app = App::new(dir.join("Library"), dir.join("data"));
        // narrow terminal: path must shrink but keep ≥1 space before stats
        let backend = ratatui::backend::TestBackend::new(40, 10);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        let buf = term.backend().buffer().clone();
        let row: String = (0..40).map(|x| buf[(x, 9)].symbol().to_string()).collect();
        let trimmed_end = row.trim_end().to_string();
        // at least one space separating a non-empty path from stats
        if trimmed_end.contains("saved")
            || trimmed_end.contains("words")
            || trimmed_end.contains("typed")
        {
            let stats_at = trimmed_end
                .find("saved")
                .or_else(|| trimmed_end.find("words"))
                .or_else(|| trimmed_end.find("typed"))
                .or_else(|| trimmed_end.find("net"))
                .unwrap_or(trimmed_end.len());
            if stats_at > 0 {
                assert!(
                    trimmed_end[..stats_at].ends_with(' '),
                    "no gap before stats: {trimmed_end:?}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn panels_show_hints_instead_of_path() {
        use crate::app::Focus;
        let (mut app, dir) = hint_app("panels");
        let backend = ratatui::backend::TestBackend::new(160, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();

        app.focus = Focus::Library;
        term.draw(|f| render(f, &mut app)).unwrap();
        let row = bottom_row(&term, 160, 24);
        assert!(row.contains("rename"), "library hints: {row:?}");
        assert!(row.contains("X mark"), "library hints: {row:?}");
        assert!(!row.contains("alcott-hint-panels"), "no path: {row:?}");

        app.focus = Focus::Sheets;
        term.draw(|f| render(f, &mut app)).unwrap();
        let row = bottom_row(&term, 160, 24);
        assert!(row.contains("reorder"), "sheets hints: {row:?}");

        // editor keeps the path
        app.focus = Focus::Editor;
        term.draw(|f| render(f, &mut app)).unwrap();
        let row = bottom_row(&term, 160, 24);
        assert!(row.contains("alcott-hint-panels"), "editor path: {row:?}");
        assert!(!row.contains("reorder"), "editor path: {row:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn narrow_panels_fall_back_to_minimal_hints() {
        use crate::app::Focus;
        let (mut app, dir) = hint_app("narrow");
        // right side ~= 44 cols, so budget ~25 → minimal tier
        let backend = ratatui::backend::TestBackend::new(70, 12);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        app.focus = Focus::Library;
        term.draw(|f| render(f, &mut app)).unwrap();
        let row = bottom_row(&term, 70, 12);
        assert!(row.contains("X mark"), "minimal hints: {row:?}");
        assert!(!row.contains("rename"), "no room for full: {row:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_uses_terminal_defaults_only() {
        use crate::config::theme;
        use ratatui::style::Color;
        for s in [
            theme::base(),
            theme::dim(),
            theme::bold(),
            theme::selected(),
            theme::accel(),
            theme::accel_dim(),
            theme::md_sym(),
            theme::md_marker(),
            theme::md_code(),
            theme::md_link_text(),
        ] {
            assert!(
                s.fg.is_none() || s.fg == Some(Color::Reset),
                "custom fg in {s:?}"
            );
            assert!(
                s.bg.is_none() || s.bg == Some(Color::Reset),
                "custom bg in {s:?}"
            );
        }
    }

    #[test]
    fn rendered_buffer_has_no_custom_colors() {
        use crate::app::{Focus, MenuKind};
        use ratatui::style::Color;
        let (mut app, dir) = hint_app("nocolor");
        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        let check = |term: &ratatui::Terminal<ratatui::backend::TestBackend>, label: &str| {
            for cell in term.backend().buffer().content.iter() {
                assert_eq!(cell.fg, Color::Reset, "{label}");
                assert_eq!(cell.bg, Color::Reset, "{label}");
            }
        };
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "base");
        for kind in [MenuKind::File, MenuKind::Edit, MenuKind::View, MenuKind::Export] {
            app.toggle_menu(kind);
            term.draw(|f| render(f, &mut app)).unwrap();
            check(&term, "menu");
        }
        app.close_menu();
        app.show_help = true;
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "help");
        app.show_help = false;
        app.show_guide = true;
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "guide");
        app.show_guide = false;
        app.new_prompt = Some(crate::app::NewSheetPrompt::default());
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "sheet prompt");
        app.new_prompt = None;
        app.group_prompt = Some(crate::app::GroupPrompt {
            kind: crate::app::GroupPromptKind::New {
                parent: std::path::PathBuf::new(),
            },
            input: String::new(),
        });
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "group prompt");
        app.group_prompt = None;
        app.open_export();
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "export dialog");
        app.export_preview = true;
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "export preview");
        app.export_preview = false;
        app.export_prompt = Some(crate::app::ExportPrompt {
            kind: crate::app::ExportPromptKind::OutputPath,
            input: String::new(),
        });
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "export prompt");
        app.export_prompt = None;
        app.close_export();
        app.focus = Focus::Library;
        app.show_panels = false;
        term.draw(|f| render(f, &mut app)).unwrap();
        check(&term, "hidden panels");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn custom_split_reaches_panes() {
        let (mut app, dir) = hint_app("splitw");
        app.lib_frac = 0.4;
        app.sheets_frac = 0.2;
        let backend = ratatui::backend::TestBackend::new(120, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        assert_eq!(app.lib_area.map(|r| r.width), Some(48));
        assert_eq!(app.sheets_area.map(|r| r.width), Some(24));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_banner_replaces_hints() {
        use crate::app::{Focus, MoveLift};
        let (mut app, dir) = hint_app("mvban");
        app.focus = Focus::Sheets;
        app.move_lift = Some(MoveLift {
            groups: vec![],
            sheets: vec![std::path::PathBuf::from("/tmp/x.md")],
        });
        let backend = ratatui::backend::TestBackend::new(160, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        let row = bottom_row(&term, 160, 24);
        assert!(row.contains("MOVE"), "banner: {row:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn all_rows(term: &ratatui::Terminal<ratatui::backend::TestBackend>, w: u16, h: u16) -> Vec<String> {
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| term.backend().buffer()[(x, y)].symbol().to_string())
                    .collect()
            })
            .collect()
    }

    fn seed_tree(lib: &std::path::Path) {
        crate::library::Library::create_group(lib, std::path::Path::new(""), "Zed").unwrap();
        crate::library::Library::create_group(
            lib,
            &std::path::PathBuf::from("Zed"),
            "Sub",
        )
        .unwrap();
        crate::library::Library::create_sheet(lib, "Zed", "Card").unwrap();
        std::fs::write(
            lib.join("Zed").join("Card.md"),
            "# Card\n\npreview one\npreview two\n",
        )
        .unwrap();
    }

    #[test]
    fn library_uses_arrow_glyphs() {
        use crate::app::Focus;
        let (mut app, dir) = hint_app("arrows");
        seed_tree(&dir.join("Library"));
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Library;
        let backend = ratatui::backend::TestBackend::new(120, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        let rows = all_rows(&term, 120, 24);
        let zed = rows.iter().find(|r| r.contains("Zed")).expect("Zed row");
        assert!(zed.contains('▾'), "open arrow: {zed:?}");
        assert!(!zed.contains('V'), "no V glyph: {zed:?}");
        // collapse the top group: arrow points sideways
        app.library.toggle(&std::path::PathBuf::from("Zed"));
        term.draw(|f| render(f, &mut app)).unwrap();
        let rows = all_rows(&term, 120, 24);
        let zed = rows.iter().find(|r| r.contains("Zed")).expect("Zed row");
        assert!(zed.contains('▸'), "closed arrow: {zed:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn selected_library_row_fills_panel_width() {
        use crate::app::Focus;
        use ratatui::style::Modifier;
        let (mut app, dir) = hint_app("libcard");
        seed_tree(&dir.join("Library"));
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Library;
        app.lib_cursor = 0;
        let backend = ratatui::backend::TestBackend::new(120, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        let area = app.lib_area.expect("lib rect recorded");
        let buf = term.backend().buffer();
        // selected row: every cell across the inner width is reversed
        let sel_y = app.lib_rows[0].0;
        for x in area.x..area.x + area.width - 1 {
            assert!(
                buf[(x, sel_y)].modifier.contains(Modifier::REVERSED),
                "card fill at x={x}"
            );
        }
        // an unselected row keeps default trailing cells
        let other_y = app.lib_rows[1].0;
        let tail_x = area.x + area.width - 2;
        assert!(
            !buf[(tail_x, other_y)].modifier.contains(Modifier::REVERSED),
            "unselected row stays plain"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn selected_sheet_block_is_full_width_card() {
        use crate::app::Focus;
        use ratatui::style::Modifier;
        let (mut app, dir) = hint_app("sheetcard");
        seed_tree(&dir.join("Library"));
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Sheets;
        app.sheet_cursor = 0;
        let backend = ratatui::backend::TestBackend::new(120, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        let area = app.sheets_area.expect("sheets rect recorded");
        let buf = term.backend().buffer();
        // title row + preview row(s) + separator row: full inner width reversed.
        // (previews join into one wrapped line, so the block is 3 rows here)
        let title_y = app.sheet_rows[0].0;
        for dy in 0..3u16 {
            for x in area.x..area.x + area.width - 1 {
                assert!(
                    buf[(x, title_y + dy)].modifier.contains(Modifier::REVERSED),
                    "card fill at ({x},{})",
                    title_y + dy
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_draws_its_own_cursor() {
        use crate::app::Focus;
        use ratatui::style::Modifier;
        let (mut app, dir) = hint_app("cursor");
        let lib = dir.join("Library");
        crate::library::Library::create_group(&lib, std::path::Path::new(""), "G").unwrap();
        crate::library::Library::create_sheet(&lib, "G", "Cur").unwrap();
        app.library = crate::library::Library::scan(&lib);
        app.focus = Focus::Editor;
        app.open_selected_sheet();
        app.focus = Focus::Editor;
        let backend = ratatui::backend::TestBackend::new(120, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        // the char under the cursor ('#', fresh sheet) is reverse-video
        let found = term
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.symbol() == "#" && c.modifier.contains(Modifier::REVERSED));
        assert!(found, "drawn cursor on first char");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn row_with<'a>(
        term: &'a ratatui::Terminal<ratatui::backend::TestBackend>,
        w: u16,
        h: u16,
        needle: &str,
    ) -> Option<(u16, String)> {
        (0..h).find_map(|y| {
            let row: String = (0..w)
                .map(|x| term.backend().buffer()[(x, y)].symbol().to_string())
                .collect();
            row.contains(needle).then(|| (y, row))
        })
    }

    fn row_has_modifier(
        term: &ratatui::Terminal<ratatui::backend::TestBackend>,
        mut xs: std::ops::Range<u16>,
        y: u16,
        m: ratatui::style::Modifier,
    ) -> bool {
        xs.any(|x| term.backend().buffer()[(x, y)].modifier.contains(m))
    }

    #[test]
    fn breadcrumb_trail_marks_open_location() {
        use crate::app::Focus;
        use ratatui::style::Modifier;
        let (mut app, dir) = hint_app("crumb");
        let lib = dir.join("Library");
        for g in ["G1", "G2"] {
            crate::library::Library::create_group(&lib, std::path::Path::new(""), g).unwrap();
            crate::library::Library::create_sheet(&lib, g, "S1").unwrap();
            crate::library::Library::create_sheet(&lib, g, "S2").unwrap();
        }
        app.library = crate::library::Library::scan(&lib);
        // open S1 in G1
        app.focus = Focus::Sheets;
        app.sheet_cursor = 0;
        app.open_selected_sheet();
        let backend = ratatui::backend::TestBackend::new(120, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();

        // editing: both the group and the sheet show reverse cards
        app.focus = Focus::Editor;
        term.draw(|f| render(f, &mut app)).unwrap();
        let la = app.lib_area.expect("lib area");
        let sa = app.sheets_area.expect("sheets area");
        let libr = la.x..la.x + la.width - 1;
        let sheer = sa.x..sa.x + sa.width - 1;
        let (gy, _) = row_with(&term, 120, 24, "G1").expect("G1 row");
        for x in libr.clone() {
            assert!(
                term.backend().buffer()[(x, gy)].modifier.contains(Modifier::REVERSED),
                "G1 card at x={x}"
            );
        }
        let (sy, _) = row_with(&term, 120, 24, "S1.md").expect("S1 row");
        for x in sheer.clone() {
            assert!(
                term.backend().buffer()[(x, sy)].modifier.contains(Modifier::REVERSED),
                "S1 card at x={x}"
            );
        }
        // navigating elsewhere: cursor owns the cards, old location is plain
        app.focus = Focus::Library;
        app.lib_cursor = 1;
        term.draw(|f| render(f, &mut app)).unwrap();
        for x in libr.clone() {
            assert!(
                !term.backend().buffer()[(x, gy)].modifier.contains(Modifier::REVERSED),
                "G1 plain while browsing G2"
            );
        }
        let (gy2, _) = row_with(&term, 120, 24, "G2").expect("G2 row");
        assert!(
            row_has_modifier(&term, libr, gy2, Modifier::REVERSED),
            "cursor card on G2"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sheets_panel_keeps_cursor_in_view() {
        use crate::app::Focus;
        let (mut app, dir) = hint_app("sheetscroll");
        let lib = dir.join("Library");
        crate::library::Library::create_group(&lib, std::path::Path::new(""), "Big").unwrap();
        for k in 0..20 {
            std::fs::write(
                lib.join("Big").join(format!("Sheet{k:02}.md")),
                format!("# Sheet{k:02}\n\npreview one {k}\npreview two {k}\n"),
            )
            .unwrap();
        }
        app.library = crate::library::Library::scan(&lib);
        app.focus = Focus::Sheets;
        // Jump to the last sheet: the panel is far too short to show it
        // without scrolling (each block is 4 rows incl. gap).
        app.sheet_cursor = 19;
        let backend = ratatui::backend::TestBackend::new(120, 12);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        assert!(app.sheet_scroll > 0, "view should have scrolled");
        assert!(
            app.sheet_rows.iter().any(|(_, i)| *i == 19),
            "cursor row must be mapped for clicks"
        );
        let rows = all_rows(&term, 120, 12);
        assert!(
            rows.iter().any(|r| r.contains("Sheet19")),
            "last sheet must be visible"
        );
        assert!(
            !rows.iter().any(|r| r.contains("Sheet00")),
            "first sheet should have scrolled out"
        );
        // Back up: the view follows the cursor home.
        app.sheet_cursor = 0;
        term.draw(|f| render(f, &mut app)).unwrap();
        assert_eq!(app.sheet_scroll, 0);
        let rows = all_rows(&term, 120, 12);
        assert!(rows.iter().any(|r| r.contains("Sheet00")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_panel_keeps_cursor_in_view() {
        use crate::app::Focus;
        let (mut app, dir) = hint_app("libscroll");
        let lib = dir.join("Library");
        for k in 0..30 {
            crate::library::Library::create_group(
                &lib,
                std::path::Path::new(""),
                &format!("G{k:02}"),
            )
            .unwrap();
        }
        app.library = crate::library::Library::scan(&lib);
        app.focus = Focus::Library;
        app.lib_cursor = 29;
        let backend = ratatui::backend::TestBackend::new(120, 12);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        assert!(app.lib_scroll > 0, "view should have scrolled");
        assert!(app.lib_rows.iter().any(|(_, i)| *i == 29));
        let rows = all_rows(&term, 120, 12);
        assert!(rows.iter().any(|r| r.contains("G29")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn breadcrumb_covers_multiview_groups() {
        use crate::app::Focus;
        use ratatui::style::Modifier;
        let (mut app, dir) = hint_app("crumbmv");
        let lib = dir.join("Library");
        for g in ["G1", "G2"] {
            crate::library::Library::create_group(&lib, std::path::Path::new(""), g).unwrap();
            crate::library::Library::create_sheet(&lib, g, "S1").unwrap();
        }
        app.library = crate::library::Library::scan(&lib);
        app.focus = Focus::Sheets;
        // mark S1 in G1 then S1 in G2 → multi-view across groups
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        app.lib_cursor = 1;
        app.sheet_cursor = 0;
        app.toggle_mark_sheet();
        assert!(app.multiview.is_some());
        let backend = ratatui::backend::TestBackend::new(120, 24);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        // editing a multi-view: every containing group gets the card
        app.focus = Focus::Editor;
        term.draw(|f| render(f, &mut app)).unwrap();
        let la = app.lib_area.expect("lib area");
        let libr = la.x..la.x + la.width - 1;
        for g in ["G1", "G2"] {
            let (gy, _) = row_with(&term, 120, 24, g).expect("group row");
            for x in libr.clone() {
                assert!(
                    term.backend().buffer()[(x, gy)]
                        .modifier
                        .contains(Modifier::REVERSED),
                    "{g} card at x={x}"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    fn seed_big_doc(lib: &std::path::Path, name: &str, lines: usize) -> std::path::PathBuf {
        crate::library::Library::create_group(lib, std::path::Path::new(""), "G").unwrap();
        let mut text = String::from("# Big\n\n");
        for k in 0..lines {
            text.push_str(&format!("Line {k:03} alpha beta gamma delta epsilon zeta eta theta.\n"));
        }
        let abs = lib.join("G").join(name);
        std::fs::write(&abs, &text).unwrap();
        abs
    }

    #[test]
    fn editor_window_keeps_cursor_visible() {
        use crate::app::Focus;
        use ratatui::style::Modifier;
        let (mut app, dir) = hint_app("edwin");
        let lib = dir.join("Library");
        let abs = seed_big_doc(&lib, "Doc.md", 200);
        app.library = crate::library::Library::scan(&lib);
        app.focus = Focus::Sheets;
        app.sheet_cursor = 0;
        app.open_selected_sheet();
        assert_eq!(app.editor.as_ref().unwrap().path, abs);
        // jump deep into the file
        let target = app.editor.as_ref().unwrap().rope.line_to_char(150);
        app.editor.as_mut().unwrap().cursor = target;
        app.focus = Focus::Editor;
        let backend = ratatui::backend::TestBackend::new(120, 12);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        // the cursor's line is mapped and visible ...
        assert!(
            app.editor_map.iter().any(|r| r.line == 150),
            "cursor line mapped"
        );
        // ... and the fake cursor is drawn (reverse video somewhere).
        let found = term
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.modifier.contains(Modifier::REVERSED));
        assert!(found, "drawn cursor visible");
        // the window scrolled: it no longer starts at the top.
        assert!(app.editor_anchor > 0 || app.editor_scroll > 0);
        // every mapped click lands inside the buffer.
        let inner = app.editor_inner.expect("inner rect");
        let len = app.editor.as_ref().unwrap().rope.len_chars();
        for (dy, row) in app.editor_map.clone().iter().enumerate() {
            let pos = app
                .editor_point_to_char(inner.x, inner.y + dy as u16)
                .expect("mapped click hits");
            let expect = app.editor.as_ref().unwrap().rope.line_to_char(row.line) + row.char_start;
            assert_eq!(pos, expect.min(len), "row {dy}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_wheel_scrolls_and_trims() {
        use crate::app::Focus;
        let (mut app, dir) = hint_app("edwheel");
        let lib = dir.join("Library");
        seed_big_doc(&lib, "Doc.md", 200);
        app.library = crate::library::Library::scan(&lib);
        app.focus = Focus::Sheets;
        app.sheet_cursor = 0;
        app.open_selected_sheet();
        app.focus = Focus::Editor;
        app.editor_inner = Some(ratatui::layout::Rect::new(60, 2, 50, 10));
        app.ed_wheel(false);
        app.ed_wheel(false);
        assert_eq!(app.editor_scroll, 6);
        app.ed_wheel(true);
        app.ed_wheel(true);
        assert_eq!(app.editor_scroll, 0);
        assert_eq!(app.editor_anchor, 0);
        // wheel far down, then render: the window trims back to a viewport.
        for _ in 0..100 {
            app.ed_wheel(false);
        }
        assert_eq!(app.editor_scroll, 300);
        let backend = ratatui::backend::TestBackend::new(120, 12);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        assert!(app.editor_anchor > 0, "window advanced past the top");
        assert!(
            (app.editor_scroll as usize) < 4 * app.editor_inner.unwrap().height as usize + 10,
            "scroll trimmed, got {}",
            app.editor_scroll
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn editor_click_follows_wrapped_rows() {
        use crate::app::Focus;
        let (mut app, dir) = hint_app("edclick");
        let lib = dir.join("Library");
        crate::library::Library::create_group(&lib, std::path::Path::new(""), "G").unwrap();
        // long second line wraps at narrow widths
        std::fs::write(
            lib.join("G").join("Doc.md"),
            "# T\n\nabcdefghij klmnopqrstuvwxyz 0123456789\n",
        )
        .unwrap();
        app.library = crate::library::Library::scan(&lib);
        app.focus = Focus::Sheets;
        app.sheet_cursor = 0;
        app.open_selected_sheet();
        app.focus = Focus::Editor;
        // narrow terminal so the long line wraps to several visual rows
        let backend = ratatui::backend::TestBackend::new(60, 12);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| render(f, &mut app)).unwrap();
        let inner = app.editor_inner.expect("inner rect");
        // first visual row of the wrapped line maps to its start
        let l2 = app.editor.as_ref().unwrap().rope.line_to_char(2);
        let mut saw_continuation = false;
        for (dy, row) in app.editor_map.clone().iter().enumerate() {
            if row.line == 2 && row.char_start > 0 {
                let pos = app
                    .editor_point_to_char(inner.x, inner.y + dy as u16)
                    .expect("continuation click hits");
                assert_eq!(pos, l2 + row.char_start, "continuation row {dy}");
                saw_continuation = true;
            }
        }
        assert!(saw_continuation, "long line wrapped");
        let _ = std::fs::remove_dir_all(&dir);
    }

}
