mod app;
mod buffer;
mod cli;
mod config;
mod export;
mod library;
mod md_style;
mod order;
mod stats;
mod ui;
mod view;
mod words;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Parser;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use app::{App, Focus};

fn main() -> anyhow::Result<()> {
    let cli = cli::Cli::parse();
    let library_root = config::resolve_library_root(cli.library.clone());
    let data_dir = config::data_dir();

    match cli.cmd {
        Some(cli::Cmd::New { title, group }) => {
            config::ensure_dirs(&library_root, &data_dir)?;
            let abs = library::Library::create_sheet(&library_root, &group, &title)?;
            println!("{}", abs.display());
            Ok(())
        }
        Some(cli::Cmd::List { group }) => {
            let lib = library::Library::scan(&library_root);
            if let Some(g) = group {
                let gp = PathBuf::from(&g);
                let sheets = lib.sheets_for_group(Some(&gp));
                for s in sheets {
                    println!("{}", s.rel.display());
                }
            } else {
                for g in &lib.groups {
                    println!("{}/", g.rel.display());
                }
                for s in &lib.sheets {
                    println!("{}", s.rel.display());
                }
            }
            Ok(())
        }
        Some(cli::Cmd::Print { target }) => {
            let found = resolve_sheet(&library_root, &target)?;
            let mut out = std::io::stdout().lock();
            let bytes = std::fs::read(found)?;
            out.write_all(&bytes)?;
            Ok(())
        }
        Some(cli::Cmd::Search { query, group }) => {
            let lib = library::Library::scan(&library_root);
            let q = query.to_lowercase();
            let mut hits = 0;
            for s in &lib.sheets {
                if let Some(ref g) = group {
                    let gp = PathBuf::from(g);
                    if s.group != gp && !s.group.starts_with(&gp) {
                        continue;
                    }
                }
                let name_hit = s.name.to_lowercase().contains(&q);
                if name_hit {
                    println!("{}: <name>", s.rel.display());
                    hits += 1;
                    if hits >= 100 {
                        break;
                    }
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&s.abs) {
                    for (i, line) in text.lines().enumerate() {
                        if line.to_lowercase().contains(&q) {
                            println!("{}:{}: {}", s.rel.display(), i + 1, line.trim());
                            hits += 1;
                            if hits >= 100 {
                                break;
                            }
                            break;
                        }
                    }
                }
                if hits >= 100 {
                    break;
                }
            }
            Ok(())
        }
        Some(cli::Cmd::Export { target, group, format, style, css, out, force, font }) => {
            run_cli_export(&library_root, &data_dir, target, group, format, style, css, out, force, font)
        }
        None => {
            let mouse_override = if cli.no_mouse {
                Some(false)
            } else if cli.mouse {
                Some(true)
            } else {
                None
            };
            run_tui(library_root, data_dir, mouse_override)
        }
    }
}

fn run_cli_export(
    library_root: &Path,
    data_dir: &Path,
    target: Option<String>,
    group: Option<String>,
    format: String,
    style: String,
    css: Option<PathBuf>,
    out: Option<PathBuf>,
    force: bool,
    fonts: Vec<PathBuf>,
) -> anyhow::Result<()> {
    use crate::export::{ExportFormat, SheetDoc};
    let fmt = ExportFormat::from_extension(&format)
        .ok_or_else(|| anyhow::anyhow!("unknown format: {format} (md, html, rtf, pdf, docx, epub)"))?;
    let lib = library::Library::scan(library_root);
    // Collect sheets: --group exports the group, else single target, else error.
    let mut sheets: Vec<&library::SheetMeta> = Vec::new();
    if let Some(g) = group {
        let gp = PathBuf::from(&g);
        sheets.extend(lib.sheets_for_group(Some(&gp)));
        if sheets.is_empty() {
            anyhow::bail!("no sheets in group: {g}");
        }
    } else if let Some(t) = target {
        let found = resolve_sheet(library_root, &t)?;
        let hit = lib.sheets.iter().find(|s| s.abs == found).cloned();
        match hit {
            Some(s) => {
                // find in scan to keep borrow simple: re-scan ref
                let idx = lib.sheets.iter().position(|x| x.abs == s.abs).unwrap();
                sheets.push(&lib.sheets[idx]);
            }
            None => {
                // Outside the library (absolute path): export it directly.
                let text = std::fs::read_to_string(&found)?;
                let base = found.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| PathBuf::from("."));
                let docs = vec![SheetDoc::new(
                    found.file_stem().and_then(|s| s.to_str()).unwrap_or("sheet").to_string(),
                    text,
                    base,
                )];
                return write_cli_docs(&docs, data_dir, &fmt, &style, css, out, force, fonts);
            }
        }
    } else {
        anyhow::bail!("provide a TARGET sheet or --group GROUP");
    }
    let mut docs = Vec::new();
    for s in sheets {
        let content = std::fs::read_to_string(&s.abs).unwrap_or_default();
        let base = s.abs.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| library_root.to_path_buf());
        let hint = s.abs.file_stem().and_then(|x| x.to_str()).unwrap_or("sheet").to_string();
        docs.push(SheetDoc::new(hint, content, base));
    }
    write_cli_docs(&docs, data_dir, &fmt, &style, css, out, force, fonts)
}

fn write_cli_docs(
    docs: &[crate::export::SheetDoc],
    data_dir: &Path,
    fmt: &crate::export::ExportFormat,
    style: &str,
    css: Option<PathBuf>,
    out: Option<PathBuf>,
    force: bool,
    fonts: Vec<PathBuf>,
) -> anyhow::Result<()> {
    use crate::export as ex;
    // CSS: explicit file wins, else named style (re-read from disk like the TUI).
    let css_text = if let Some(p) = css {
        std::fs::read_to_string(&p).map_err(|e| anyhow::anyhow!("could not read --css {}: {e}", p.display()))?
    } else {
        ex::css_for_style(data_dir, style).0
    };
    let attached: Vec<String> = fonts.iter().map(|p| p.to_string_lossy().to_string()).collect();
    // Validate attached fonts up front (same warnings as the TUI).
    let (_, font_problems) = ex::attached_font_map(&attached);
    for w in &font_problems {
        eprintln!("warning: {w}");
    }
    let titles: Vec<String> = docs.iter().map(|d| ex::inferred_title(&d.content)).collect();
    let stem = ex::export_stem_for_titles(&titles);
    let bytes: Vec<u8> = match fmt {
        ex::ExportFormat::Md => ex::export_markdown(docs).into_bytes(),
        ex::ExportFormat::Html => ex::export_html(docs, &css_text, &stem).into_bytes(),
        ex::ExportFormat::Rtf => {
                let (s, warnings) = ex::export_rtf(docs, &css_text, &attached);
                for w in &warnings {
                    eprintln!("warning: {w}");
                }
                s.into_bytes()
            }
            ex::ExportFormat::Docx => {
                let (b, warnings) = ex::export_docx_bytes(docs, &css_text, &attached);
                for w in &warnings {
                    eprintln!("warning: {w}");
                }
                b
            }
        ex::ExportFormat::Epub => ex::export_epub_bytes(docs, &css_text, &stem),
        ex::ExportFormat::Pdf => {
            let (b, warnings) = ex::export_pdf_bytes(docs, &css_text, &attached);
            for w in &warnings {
                eprintln!("warning: {w}");
            }
            b
        }
    };
    if bytes.is_empty() {
        anyhow::bail!("export produced no bytes");
    }
    let requested = out.unwrap_or_else(|| PathBuf::from(format!("{stem}.{}", fmt.extension())));
    // Conflict handling: never blind-overwrite unless --force.
    if requested.exists() && !force {
        let alt = ex::unique_output_path(&requested, false);
        std::fs::create_dir_all(alt.parent().unwrap_or_else(|| Path::new(".")))?;
        std::fs::write(&alt, &bytes)?;
        println!("exists: {} — wrote {} instead (use --force to overwrite)", requested.display(), alt.display());
        return Ok(());
    }
    if let Some(parent) = requested.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(&requested, &bytes)?;
    println!("exported {} sheet(s) to {} ({})", docs.len(), requested.display(), ex::human_size(bytes.len()));
    Ok(())
}

fn resolve_sheet(root: &Path, target: &str) -> anyhow::Result<PathBuf> {
    let p = PathBuf::from(target);
    if p.is_absolute() && p.exists() {
        return Ok(p);
    }
    let joined = root.join(&p);
    if joined.exists() {
        return Ok(joined);
    }
    // fuzzy: match by file name or rel suffix
    let lib = library::Library::scan(root);
    let q = target.to_lowercase();
    let mut exact: Option<PathBuf> = None;
    let mut partial: Option<PathBuf> = None;
    for s in &lib.sheets {
        if s.name.to_lowercase() == q || s.rel.to_string_lossy().to_lowercase() == q {
            exact = Some(s.abs.clone());
            break;
        }
        if s.name.to_lowercase().contains(&q)
            || s.rel.to_string_lossy().to_lowercase().contains(&q)
        {
            partial = partial.or(Some(s.abs.clone()));
        }
    }
    exact
        .or(partial)
        .ok_or_else(|| anyhow::anyhow!("sheet not found: {target}"))
}

fn set_mouse_capture(on: bool) {
    if on {
        let _ = crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    } else {
        let _ = crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    }
}

fn run_tui(
    library_root: PathBuf,
    data_dir: PathBuf,
    mouse_override: Option<bool>,
) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    crossterm::execute!(stdout, crossterm::terminal::EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(stdout);
    let mut terminal = ratatui::Terminal::new(backend)?;
    terminal.clear()?;

    let mut app = App::new(library_root, data_dir);
    if let Some(on) = mouse_override {
        app.apply_mouse_override(on);
    }
    if app.mouse_enabled {
        set_mouse_capture(true);
    }

    let mut first = true;
    loop {
        // Cursor visibility is driven solely by `frame.set_cursor_position`
        // during render (ratatui hides the cursor when no position is set):
        // visible in text prompts, hidden everywhere else. The editor draws
        // its own reverse-video cursor.
        // Idle poll timeouts skip rendering: full-doc style + wrap only run
        // when input arrived or a background tick did real work (save/stats).
        let had_event = event::poll(Duration::from_millis(200))?;
        if had_event {
            match event::read()? {
                Event::Key(k) => handle_key(&mut app, k.code, k.modifiers),
                Event::Mouse(m) => handle_mouse(&mut app, m),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
        let saved = app.maybe_autosave();
        let tick_work = saved || app.maybe_refresh_stats(false);
        if first || had_event || tick_work {
            terminal.draw(|f| ui::render(f, &mut app))?;
            first = false;
        }
        if app.should_quit {
            // save on quit if dirty
            if app.ed_is_dirty() {
                app.save();
            }
            app.cleanup_zathura_tmp();
            break;
        }
    }

    set_mouse_capture(false);
    disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn activate_menu_item(app: &mut App, kind: crate::app::MenuKind, idx: usize) {
    use crate::app::MenuKind;
    match kind {
        MenuKind::File => match idx {
            // 0 New sheet, 1 New group, 2 Save, 3 Delete sheet, 4 Quit
            0 => {
                app.new_prompt = Some(crate::app::NewSheetPrompt::default());
            }
            1 => {
                app.start_new_group();
            }
            2 => {
                app.close_menu();
                app.save();
            }
            3 => {
                app.close_menu();
                app.delete_current_sheet();
            }
            _ => {
                app.should_quit = true;
            }
        },
        MenuKind::Edit => match idx {
            // 0 Focus editor, 1 Delete line, 2 Find in sheet, 3 Undo, 4 Redo
            0 => {
                app.close_menu();
                app.focus = Focus::Editor;
            }
            1 => {
                app.close_menu();
                if app.focus != Focus::Editor {
                    app.focus = Focus::Editor;
                }
                if let Some(ed) = app.editor.as_mut() {
                    ed.delete_current_line();
                }
                app.on_edit_no_word();
            }
            2 => {
                app.close_menu();
                app.start_search();
            }
            3 => {
                app.close_menu();
                app.ed_undo();
            }
            4 => {
                app.close_menu();
                app.ed_redo();
            }
            _ => {
                app.close_menu();
                if app.focus != Focus::Editor {
                    app.focus = Focus::Editor;
                }
                if let Some(ed) = app.editor.as_mut() {
                    ed.delete_current_line();
                }
                app.on_edit_no_word();
            }
        },
        MenuKind::View => match idx {
            // 0 Library, 1 Sheets, 2 Editor, 3 Toggle panels, 4 Guide, 5 Help
            0 => {
                app.close_menu();
                if app.show_panels {
                    app.focus = Focus::Library;
                } else {
                    app.focus = Focus::Editor;
                }
            }
            1 => {
                app.close_menu();
                if app.show_panels {
                    app.focus = Focus::Sheets;
                } else {
                    app.focus = Focus::Editor;
                }
            }
            2 => {
                app.close_menu();
                app.focus = Focus::Editor;
            }
            3 => {
                app.close_menu();
                app.toggle_panels();
            }
            4 => {
                app.close_menu();
                app.show_guide = true;
            }
            5 => {
                app.close_menu();
                let on = app.toggle_mouse();
                set_mouse_capture(on);
            }
            _ => {
                app.close_menu();
                app.show_help = true;
            }
        },
        MenuKind::Export => match idx {
            // 0 Save, 1 Preview, 2 Open, 3 Next style, 4 Next format, 5 Manage
            0 => {
                app.close_menu();
                app.open_export();
                app.save_export();
            }
            1 => {
                app.close_menu();
                app.open_export();
                if !matches!(
                    app.export_format,
                    crate::export::ExportFormat::Pdf | crate::export::ExportFormat::Epub
                ) {
                    app.export_preview = true;
                } else if !app.preview_pdf_epub_zathura() {
                    // Fallback overlay already armed inside with a status note.
                }
            }
            2 => {
                app.close_menu();
                app.open_export();
                app.open_export_external();
            }
            3 => {
                app.close_menu();
                app.open_export();
                let names = app.export_style_names();
                if !names.is_empty() {
                    let next = (app.export_style_cursor + 1) % names.len();
                    let name = names[next].clone();
                    app.apply_export_style(&name);
                }
            }
            4 => {
                app.close_menu();
                app.open_export();
                let order = crate::export::ExportFormat::grid_order();
                let next = (app.export_format_cursor + 1) % order.len();
                app.apply_export_format(order[next]);
            }
            _ => {
                app.close_menu();
                app.open_export();
            }
        },
    }
}

fn handle_mouse(app: &mut App, m: MouseEvent) {
    if !app.mouse_enabled {
        return;
    }
    match m.kind {
        MouseEventKind::ScrollUp => scroll_hovered(app, m.column, m.row, true),
        MouseEventKind::ScrollDown => scroll_hovered(app, m.column, m.row, false),
        MouseEventKind::Moved => {
            // Hover-highlight menu rows while a menu is open.
            if app.open_menu.is_some() {
                if let Some(area) = app.menu_area {
                    if m.column >= area.x && m.column < area.x.saturating_add(area.width) {
                        if let Some(idx) = app.menu_index_at(m.row) {
                            app.menu_cursor = idx;
                        }
                    }
                }
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            mouse_click(app, m.column, m.row);
        }
        MouseEventKind::Drag(_) => {
            // Separator drag: separator rows would otherwise select panes.
            if app.drag.is_some() {
                app.drag_to(m.column);
            }
        }
        MouseEventKind::Up(_) => {
            app.end_drag(false);
        }
        _ => {}
    }
}

fn mouse_click(app: &mut App, x: u16, y: u16) {
    // Prompts: any click cancels.
    if app.new_prompt.is_some() {
        app.new_prompt = None;
        app.status = "cancelled".to_string();
        return;
    }
    if app.group_prompt.is_some() {
        app.group_prompt = None;
        app.status = "cancelled".to_string();
        return;
    }
    if app.search_prompt.is_some() {
        app.search_prompt = None;
        app.search_last = None;
        app.status = "cancelled".to_string();
        return;
    }
    if app.show_help || app.show_guide {
        app.show_help = false;
        app.show_guide = false;
        return;
    }
    // Top bar: clicking File / Edit / View toggles its menu (switches too).
    for (kind, rect) in app.menu_bar.clone() {
        if App::rect_contains(&rect, x, y) {
            app.toggle_menu(kind);
            return;
        }
    }
    // Menus: click a row to run it, click elsewhere to close.
    if let Some(kind) = app.open_menu {
        if let Some(area) = app.menu_area {
            if App::rect_contains(&area, x, y) {
                if let Some(idx) = app.menu_index_at(y) {
                    activate_menu_item(app, kind, idx);
                }
                return;
            }
        }
        app.close_menu();
        return;
    }
    // Separators: grab to resize (checked before pane clicks).
    if let Some(sep) = app.separator_at(x, y) {
        app.start_drag(sep);
        return;
    }
    match app.pane_at(x, y) {
        Some(Focus::Library) => {
            app.focus = Focus::Library;
            if let Some(i) = app.group_at_y(y) {
                app.lib_cursor = i;
                app.sheet_cursor = 0;
                app.clear_group_pending();
            }
        }
        Some(Focus::Sheets) => {
            if let Some(i) = app.sheet_at_y(y) {
                app.sheet_cursor = i;
                app.open_selected_sheet();
            } else {
                app.focus = Focus::Sheets;
            }
        }
        Some(Focus::Editor) => {
            app.focus = Focus::Editor;
            if app.multiview.is_some() {
                if let Some((seg, c)) = app.editor_point_to_mv(x, y) {
                    if let Some(mv) = app.multiview.as_mut() {
                        if seg < mv.segments.len() {
                            mv.cursor = (seg, c.min(mv.segments[seg].rope.len_chars()));
                        }
                    }
                }
            } else if let Some(pos) = app.editor_point_to_char(x, y) {
                if let Some(ed) = app.editor.as_mut() {
                    ed.cursor = pos.min(ed.rope.len_chars());
                }
            }
        }
        None => {}
    }
}

fn scroll_hovered(app: &mut App, x: u16, y: u16, up: bool) {
    let delta: i32 = if up { -3 } else { 3 };
    match app.pane_at(x, y) {
        Some(Focus::Editor) => {
            app.ed_wheel(up);
        }
        Some(Focus::Library) => {
            let n = app.library.visible_groups().len() as i32;
            if n > 0 {
                app.lib_cursor = (app.lib_cursor as i32 + delta).clamp(0, n - 1) as usize;
                app.sheet_cursor = 0;
            }
        }
        Some(Focus::Sheets) => {
            let n = app.current_sheets().len() as i32;
            if n > 0 {
                app.sheet_cursor = (app.sheet_cursor as i32 + delta).clamp(0, n - 1) as usize;
            }
        }
        None => {}
    }
}

fn handle_export_key(app: &mut App, code: KeyCode, ctrl: bool) {
    use crate::app::ExportPromptKind;
    // Ctrl+X / Esc closes.
    if code == KeyCode::Esc {
        if app.export_preview {
            app.export_preview = false;
            return;
        }
        app.close_export();
        app.status = "export closed".to_string();
        return;
    }
    if ctrl {
        // Allow Ctrl+S to save from the dialog.
        if matches!(code, KeyCode::Char('s') | KeyCode::Char('S')) {
            app.save_export();
            return;
        }
        return;
    }
    match code {
        KeyCode::Tab | KeyCode::Char('\t') => {
            app.export_col = (app.export_col + 1) % 2;
        }
        KeyCode::Left | KeyCode::Char('h') => {
            if app.export_col > 0 {
                app.export_col -= 1;
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if app.export_col < 1 {
                app.export_col += 1;
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if app.export_col == 0 {
                app.export_style_cursor = app.export_style_cursor.saturating_sub(1);
                let names = app.export_style_names();
                if let Some(name) = names.get(app.export_style_cursor).cloned() {
                    app.apply_export_style(&name);
                }
            } else {
                let order = crate::export::ExportFormat::grid_order();
                app.export_format_cursor = app.export_format_cursor.saturating_sub(1);
                if let Some(fmt) = order.get(app.export_format_cursor).copied() {
                    app.apply_export_format(fmt);
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.export_col == 0 {
                let names = app.export_style_names();
                if !names.is_empty() {
                    app.export_style_cursor = (app.export_style_cursor + 1).min(names.len() - 1);
                    let name = names[app.export_style_cursor].clone();
                    app.apply_export_style(&name);
                }
            } else {
                let order = crate::export::ExportFormat::grid_order();
                app.export_format_cursor = (app.export_format_cursor + 1).min(order.len() - 1);
                app.apply_export_format(order[app.export_format_cursor]);
            }
        }
        KeyCode::Enter => {
            app.save_export();
        }
        KeyCode::Char('s') | KeyCode::Char('S') => app.save_export(),
        KeyCode::Char('u') | KeyCode::Char('U') => app.save_export_unique(),
        KeyCode::Char('p') | KeyCode::Char('P') => {
            if matches!(
                app.export_format,
                crate::export::ExportFormat::Pdf | crate::export::ExportFormat::Epub
            ) {
                // External live viewer replaces the overlay for PDF/ePub.
                // Missing zathura falls back to the overlay with a status note.
                app.preview_pdf_epub_zathura();
            } else {
                app.export_preview = !app.export_preview;
            }
        }
        KeyCode::Char('o') | KeyCode::Char('O') => app.open_export_external(),
        KeyCode::Char('n') | KeyCode::Char('N') => {
            app.export_prompt = Some(crate::app::ExportPrompt { kind: ExportPromptKind::NewStyle, input: String::new() });
        }
        KeyCode::Char('e') | KeyCode::Char('E') => app.edit_export_style_as_copy(),
        KeyCode::Char('i') | KeyCode::Char('I') => {
            app.export_prompt = Some(crate::app::ExportPrompt { kind: ExportPromptKind::ImportCss, input: String::new() });
        }
        KeyCode::Char('d') | KeyCode::Char('D') => {
            let name = app.export_css_name.clone();
            app.delete_export_style(&name);
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            app.export_prompt = Some(crate::app::ExportPrompt { kind: ExportPromptKind::AttachFont, input: String::new() });
        }
        KeyCode::Char('c') | KeyCode::Char('C') => app.clear_export_fonts(),
        KeyCode::Char('t') | KeyCode::Char('T') => {
            app.export_prompt = Some(crate::app::ExportPrompt { kind: ExportPromptKind::OutputPath, input: app.export_output.clone() });
        }
        _ => {}
    }
}

fn handle_key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    use crate::app::MenuKind;
    let ctrl = mods.contains(KeyModifiers::CONTROL);

    // 1) Sheet + group prompts capture all typing.
    if app.new_prompt.is_some() {
        match code {
            KeyCode::Esc => {
                app.new_prompt = None;
                app.sheet_rename_target = None;
                app.status = "cancelled".to_string();
            }
            KeyCode::Enter => {
                let title = app
                    .new_prompt
                    .as_ref()
                    .map(|p| p.input.trim().to_string())
                    .unwrap_or_default();
                app.new_prompt = None;
                if title.is_empty() {
                    app.sheet_rename_target = None;
                    app.status = "cancelled".to_string();
                } else {
                    app.commit_sheet_prompt(&title);
                }
            }
            KeyCode::Backspace => {
                if let Some(p) = app.new_prompt.as_mut() {
                    p.input.pop();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(p) = app.new_prompt.as_mut() {
                    p.input.push(c);
                }
            }
            _ => {}
        }
        return;
    }

    if app.group_prompt.is_some() {
        match code {
            KeyCode::Esc => {
                app.group_prompt = None;
                app.status = "cancelled".to_string();
            }
            KeyCode::Enter => {
                app.commit_group_prompt();
            }
            KeyCode::Backspace => {
                if let Some(p) = app.group_prompt.as_mut() {
                    p.input.pop();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(p) = app.group_prompt.as_mut() {
                    p.input.push(c);
                }
            }
            _ => {}
        }
        return;
    }

    // 1c) Find prompt captures typing; Enter jumps to the next match while
    // the prompt stays open for repeated jumps, Esc closes it.
    if app.search_prompt.is_some() {
        match code {
            KeyCode::Esc => {
                app.search_prompt = None;
                app.search_last = None;
            }
            KeyCode::Enter => {
                app.advance_search();
            }
            KeyCode::Backspace => {
                if let Some(p) = app.search_prompt.as_mut() {
                    p.input.pop();
                    app.search_last = None;
                }
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(p) = app.search_prompt.as_mut() {
                    p.input.push(c);
                    app.search_last = None;
                }
            }
            _ => {}
        }
        return;
    }

    // 2) Help / guide overlays: any key closes (export dialog stays open).
    if (app.show_help || app.show_guide) && !app.show_export {
        match code {
            KeyCode::Esc | KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Char('q')
                if !ctrl =>
            {
                app.show_help = false;
                app.show_guide = false;
                return;
            }
            KeyCode::Esc => {
                app.show_help = false;
                app.show_guide = false;
                return;
            }
            _ => {
                app.show_help = false;
                app.show_guide = false;
                return;
            }
        }
    }

    // 2b) Export text prompts capture typing (path / import / font / new style).
    if let Some(prompt) = app.export_prompt.as_mut() {
        match code {
            KeyCode::Esc => {
                app.export_prompt = None;
                app.status = "cancelled".to_string();
            }
            KeyCode::Enter => {
                let input = std::mem::take(&mut prompt.input);
                let kind = prompt.kind.clone();
                app.export_prompt = None;
                if input.trim().is_empty() {
                    app.status = "cancelled".to_string();
                } else {
                    use crate::app::ExportPromptKind;
                    match kind {
                        ExportPromptKind::OutputPath => {
                            app.export_output = input.trim().to_string();
                            app.export_confirm_overwrite = None;
                            app.status = format!("output: {}", app.export_output);
                        }
                        ExportPromptKind::ImportCss => {
                            app.import_export_style(std::path::Path::new(input.trim()));
                        }
                        ExportPromptKind::AttachFont => {
                            app.attach_export_font(std::path::Path::new(input.trim()));
                        }
                        ExportPromptKind::NewStyle => {
                            app.create_export_style(input.trim());
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                prompt.input.pop();
            }
            KeyCode::Char(c) if !ctrl => {
                prompt.input.push(c);
            }
            _ => {}
        }
        return;
    }

    // 2c) Export dialog captures most keys.
    if app.show_export {
        handle_export_key(app, code, ctrl);
        return;
    }

    // 2d) Standalone preview overlay (no export dialog): Esc dismisses it
    // without touching panes, focus, or the editor.
    if app.export_preview && code == KeyCode::Esc {
        app.export_preview = false;
        app.status = "preview closed".to_string();
        return;
    }

    // 3) Open menu: arrows navigate, Enter activates, Esc closes,
    //    accelerator letter (with or without Ctrl) activates directly.
    if let Some(kind) = app.open_menu {
        let n = app.menu_items().len();
        match code {
            KeyCode::Esc => {
                app.close_menu();
                return;
            }
            KeyCode::Up | KeyCode::Char('k') if !ctrl => {
                app.menu_cursor = app.menu_cursor.saturating_sub(1);
                return;
            }
            KeyCode::Down | KeyCode::Char('j') if !ctrl => {
                if n > 0 {
                    app.menu_cursor = (app.menu_cursor + 1).min(n - 1);
                }
                return;
            }
            KeyCode::Enter => {
                activate_menu_item(app, kind, app.menu_cursor);
                return;
            }
            KeyCode::Char(c) => {
                let lc = c.to_lowercase().next().unwrap_or(c);
                let items = app.menu_items();
                if let Some(i) = items.iter().position(|(_, a)| *a == lc) {
                    activate_menu_item(app, kind, i);
                    return;
                }
                // '.' is an alias for Toggle panels in the View menu.
                if matches!(kind, MenuKind::View) && lc == '.' {
                    if let Some(i) = items.iter().position(|(l, _)| l == "Toggle panels") {
                        activate_menu_item(app, kind, i);
                        return;
                    }
                }
                // Ctrl+F/E/V/X switches menus
                if ctrl {
                    match lc {
                        'f' => {
                            app.toggle_menu(MenuKind::File);
                            return;
                        }
                        'e' => {
                            app.toggle_menu(MenuKind::Edit);
                            return;
                        }
                        'v' => {
                            app.toggle_menu(MenuKind::View);
                            return;
                        }
                        'x' => {
                            app.toggle_menu(MenuKind::Export);
                            return;
                        }
                        _ => {}
                    }
                }
                return;
            }
            _ => {
                if ctrl {
                    return;
                }
            }
        }
        // fall through for unhandled menu keys
    }

    // 4) Global Ctrl accelerators (work from any pane; highlighted in UI).
    if ctrl {
        // Panels toggle: Ctrl+P is the reliable primary (Ctrl+. is not
        // delivered by many terminals, but kept as a fallback).
        if matches!(code, KeyCode::Char('p') | KeyCode::Char('P') | KeyCode::Char('.')) {
            app.close_menu();
            app.toggle_panels();
            return;
        }
        match code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.should_quit = true;
                return;
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                app.close_menu();
                app.save();
                return;
            }
            KeyCode::Char('f') | KeyCode::Char('F') => {
                app.toggle_menu(MenuKind::File);
                return;
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                app.toggle_menu(MenuKind::Edit);
                return;
            }
            KeyCode::Char('v') | KeyCode::Char('V') => {
                app.toggle_menu(MenuKind::View);
                return;
            }
            KeyCode::Char('x') | KeyCode::Char('X') => {
                app.close_menu();
                if app.show_export {
                    app.close_export();
                } else {
                    app.open_export();
                }
                return;
            }
            KeyCode::Char('o') | KeyCode::Char('O') => {
                // Quick preview without the export menu: external viewer
                // for PDF/ePub, in-TUI overlay otherwise.
                app.close_menu();
                app.toggle_preview();
                return;
            }
            KeyCode::Char('g') | KeyCode::Char('G') => {
                // In-buffer find from anywhere, including mid-typing.
                app.close_menu();
                app.start_search();
                return;
            }
            KeyCode::Char('k') | KeyCode::Char('K') => {
                // Edit > Delete line works from the editor
                if app.focus == Focus::Editor {
                    app.ed_delete_line();
                }
                return;
            }
            KeyCode::Char('z') | KeyCode::Char('Z') => {
                // Undo works from anywhere; it targets the open buffer.
                app.close_menu();
                app.ed_undo();
                return;
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                app.close_menu();
                app.ed_redo();
                return;
            }
            // Ctrl+1/2/3 jumps panes from anywhere — plain 1/2/3 would
            // type text while editing, so the editor has no other shortcut.
            KeyCode::Char('1') => {
                if app.show_panels {
                    app.focus = Focus::Library;
                }
                return;
            }
            KeyCode::Char('2') => {
                if app.show_panels {
                    app.focus = Focus::Sheets;
                }
                return;
            }
            KeyCode::Char('3') => {
                app.focus = Focus::Editor;
                return;
            }
            _ => {}
        }
    }

    // Alt+1/2/3 mirrors Ctrl+1/2/3: many terminals swallow Ctrl+digits
    // (legacy xterm mappings) but pass Alt+digit (ESC + digit) through.
    // Linux/Windows users typically want Alt; macOS needs Option-as-Meta.
    if mods.contains(KeyModifiers::ALT) {
        match code {
            KeyCode::Char('1') => {
                if app.show_panels {
                    app.focus = Focus::Library;
                }
                return;
            }
            KeyCode::Char('2') => {
                if app.show_panels {
                    app.focus = Focus::Sheets;
                }
                return;
            }
            KeyCode::Char('3') => {
                app.focus = Focus::Editor;
                return;
            }
            _ => {}
        }
    }

    // Hidden panels are skipped: normalize focus and route pane keys.
    if !app.show_panels
        && (app.focus == Focus::Library || app.focus == Focus::Sheets)
    {
        app.focus = Focus::Editor;
    }

    // Move mode: Esc cancels everywhere; Enter drops from Library/Sheets.
    // (Enter in the editor still types a newline.)
    if app.move_lift.is_some() {
        match code {
            KeyCode::Esc => {
                app.move_lift = None;
                app.status = "move cancelled".to_string();
                return;
            }
            KeyCode::Enter if app.focus != Focus::Editor => {
                app.drop_move();
                return;
            }
            _ => {}
        }
    }

    // Separator drag: Esc restores the pre-drag split from anywhere.
    if app.drag.is_some() && code == KeyCode::Esc {
        app.end_drag(true);
        return;
    }

    match app.focus {
        Focus::Library => match code {
            KeyCode::Char('1') => app.focus = Focus::Library,
            KeyCode::Char('2') => app.focus = Focus::Sheets,
            KeyCode::Char('3') => app.focus = Focus::Editor,
            KeyCode::Tab => app.focus = Focus::Sheets,
            KeyCode::Down | KeyCode::Char('j') => {
                let n = app.library.visible_groups().len();
                if n > 0 {
                    app.lib_cursor = (app.lib_cursor + 1).min(n - 1);
                    app.sheet_cursor = 0;
                    app.clear_group_pending();
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.lib_cursor = app.lib_cursor.saturating_sub(1);
                app.sheet_cursor = 0;
                app.clear_group_pending();
            }
            KeyCode::Char('n') if !ctrl => {
                app.close_menu();
                app.start_new_group();
            }
            KeyCode::Char('r') if !ctrl => {
                app.close_menu();
                app.start_rename_group();
            }
            KeyCode::Char('D') if !ctrl => {
                app.close_menu();
                app.delete_group_confirm();
            }
            KeyCode::Char('x') | KeyCode::Char('X') if !ctrl => {
                app.toggle_mark_group();
            }
            KeyCode::Char('m') | KeyCode::Char('M') if !ctrl => {
                app.start_move();
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter => {
                if let Some(g) = app.selected_group() {
                    if app.library.collapsed.contains(&g) {
                        app.library.toggle(&g);
                    } else if app.library.has_children(&g) {
                        // expand is default; Enter jumps to sheets
                        app.focus = Focus::Sheets;
                        app.sheet_cursor = 0;
                    } else {
                        app.focus = Focus::Sheets;
                    }
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if let Some(g) = app.selected_group() {
                    if !app.library.collapsed.contains(&g) && app.library.has_children(&g) {
                        app.library.toggle(&g);
                    }
                }
            }
            KeyCode::Char(' ') => {
                if let Some(g) = app.selected_group() {
                    app.library.toggle(&g);
                }
            }
            KeyCode::Esc => app.should_quit = true,
            _ => {}
        },
        Focus::Sheets => match code {
            KeyCode::Char('1') => app.focus = Focus::Library,
            KeyCode::Char('2') => app.focus = Focus::Sheets,
            KeyCode::Char('3') => app.focus = Focus::Editor,
            KeyCode::Tab => app.focus = Focus::Editor,
            KeyCode::Down | KeyCode::Char('j') => {
                let n = app.current_sheets().len();
                if n > 0 {
                    app.sheet_cursor = (app.sheet_cursor + 1).min(n - 1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.sheet_cursor = app.sheet_cursor.saturating_sub(1);
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => app.open_selected_sheet(),
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Esc => app.focus = Focus::Library,
            KeyCode::Char('n') if !ctrl => {
                app.close_menu();
                app.new_prompt = Some(crate::app::NewSheetPrompt::default());
            }
            KeyCode::Char('r') | KeyCode::Char('R') if !ctrl => {
                app.close_menu();
                app.start_sheet_rename();
            }
            KeyCode::Char('D') if !ctrl => {
                app.close_menu();
                app.delete_sheets_confirm();
            }
            KeyCode::Char('x') | KeyCode::Char('X') if !ctrl => {
                app.toggle_mark_sheet();
            }
            KeyCode::Char('a') | KeyCode::Char('A') if !ctrl => {
                app.toggle_mark_all_sheets();
            }
            KeyCode::Char('m') | KeyCode::Char('M') if !ctrl => {
                app.start_move();
            }
            KeyCode::Char('J') if !ctrl => {
                app.swap_sheet(1);
            }
            KeyCode::Char('K') if !ctrl => {
                app.swap_sheet(-1);
            }
            _ => {}
        },
        Focus::Editor => match code {
            KeyCode::Esc => {
                if app.show_panels {
                    app.focus = Focus::Sheets;
                }
            }
            KeyCode::Tab => {
                if app.show_panels {
                    app.focus = Focus::Library;
                }
            }
            KeyCode::Char('m') | KeyCode::Char('M') if ctrl => {
                // Ctrl+M: ignore so holding Ctrl never inserts text
            }
            KeyCode::Up => app.ed_move_up(),
            KeyCode::Down => app.ed_move_down(),
            KeyCode::Left => app.ed_move_left(ctrl),
            KeyCode::Right => app.ed_move_right(),
            KeyCode::Home => app.ed_move_home(),
            KeyCode::End => app.ed_move_end(),
            KeyCode::Backspace => app.ed_backspace(),
            KeyCode::Delete => app.ed_delete_forward(),
            KeyCode::Enter => app.ed_insert_newline(),
            KeyCode::Char(c) => {
                if ctrl {
                    // Ctrl+H = backspace, Ctrl+ arrows handled above; ignore others
                    return;
                }
                app.ed_insert_char(c);
            }
            _ => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(tag: &str) -> (App, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("alcott-key-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Library")).unwrap();
        let app = App::new(dir.join("Library"), dir.join("data"));
        (app, dir)
    }

    #[test]
    fn sheets_a_key_marks_all_in_group() {
        let (mut app, dir) = test_app("sheets-all");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S1").unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S2").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Sheets;
        app.lib_cursor = 0;
        app.sheet_cursor = 0;
        handle_key(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(app.selected_sheets.len(), 2);
        handle_key(&mut app, KeyCode::Char('A'), KeyModifiers::NONE);
        assert!(app.selected_sheets.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ctrl_o_quick_preview_toggles_overlay() {
        let (mut app, dir) = test_app("quick-preview");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Editor;
        app.open_selected_sheet();
        // Markdown format takes the in-TUI overlay path (no viewer involved).
        app.export_format = crate::export::ExportFormat::Md;
        handle_key(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
        assert!(app.export_preview);
        // The overlay renders with the export dialog closed.
        let backend = ratatui::backend::TestBackend::new(120, 30);
        let mut term = ratatui::Terminal::new(backend).unwrap();
        term.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        let text: String = term.backend().buffer().content.iter().map(|c| c.symbol().to_string()).collect();
        assert!(text.contains("Preview"), "overlay should render");
        // Second press toggles it back off.
        handle_key(&mut app, KeyCode::Char('O'), KeyModifiers::CONTROL);
        assert!(!app.export_preview);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn esc_dismisses_standalone_preview() {
        let (mut app, dir) = test_app("preview-esc");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Editor;
        app.open_selected_sheet();
        app.export_format = crate::export::ExportFormat::Md;
        handle_key(&mut app, KeyCode::Char('o'), KeyModifiers::CONTROL);
        assert!(app.export_preview);
        // Esc closes just the overlay: focus and editor untouched.
        let before = app.editor.as_ref().map(|e| e.cursor);
        handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(!app.export_preview);
        assert_eq!(app.focus, Focus::Editor);
        assert_eq!(app.editor.as_ref().map(|e| e.cursor), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_cycles_matches_and_wraps() {
        let (mut app, dir) = test_app("find-cycle");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        // line 2 holds two matches: offsets 5 and 13.
        std::fs::write(dir.join("Library").join("G").join("S.md"), "# T\n\nfoo bar foo\n").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Editor;
        app.open_selected_sheet();
        handle_key(&mut app, KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert!(app.search_prompt.is_some());
        for c in ['f', 'o', 'o'] {
            handle_key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        // typing must not touch the buffer
        assert_eq!(app.editor.as_ref().map(|e| e.cursor), Some(0));
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.editor.as_ref().map(|e| e.cursor), Some(5));
        assert!(app.status.contains("1 of 2"), "status: {}", app.status);
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.editor.as_ref().map(|e| e.cursor), Some(13));
        assert!(app.status.contains("2 of 2"), "status: {}", app.status);
        // past the last match wraps to the first
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.editor.as_ref().map(|e| e.cursor), Some(5));
        assert!(app.status.contains("wrapped"), "status: {}", app.status);
        // Esc closes the prompt; focus and cursor stay put
        handle_key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.search_prompt.is_none());
        assert_eq!(app.focus, Focus::Editor);
        assert_eq!(app.editor.as_ref().map(|e| e.cursor), Some(5));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ctrl_z_ctrl_y_undo_redo() {
        let (mut app, dir) = test_app("key-undo");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        std::fs::write(dir.join("Library").join("G").join("S.md"), "# S\n\n").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Editor;
        app.open_selected_sheet();
        let end = app.editor.as_ref().unwrap().rope.len_chars();
        app.editor.as_mut().unwrap().cursor = end;
        // Empty stack reports instead of silently doing nothing.
        handle_key(&mut app, KeyCode::Char('Z'), KeyModifiers::CONTROL);
        assert_eq!(app.status, "nothing to undo");
        // Type through the normal key path, then undo/redo via Ctrl+Z/Y.
        handle_key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        assert!(app.editor.as_ref().unwrap().rope.to_string().ends_with('x'));
        handle_key(&mut app, KeyCode::Char('Z'), KeyModifiers::CONTROL);
        assert!(app.editor.as_ref().unwrap().rope.to_string().ends_with('\n'));
        handle_key(&mut app, KeyCode::Char('Y'), KeyModifiers::CONTROL);
        assert!(app.editor.as_ref().unwrap().rope.to_string().ends_with('x'));
        // Ctrl+Z works from other panes too (targets the open buffer).
        app.focus = Focus::Sheets;
        handle_key(&mut app, KeyCode::Char('Z'), KeyModifiers::CONTROL);
        assert!(app.editor.as_ref().unwrap().rope.to_string().ends_with('\n'));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_menu_f_opens_find() {
        let (mut app, dir) = test_app("menu-find");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.open_selected_sheet();
        app.open_menu = Some(crate::app::MenuKind::Edit);
        handle_key(&mut app, KeyCode::Char('f'), KeyModifiers::NONE);
        assert!(app.search_prompt.is_some());
        assert!(app.open_menu.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_menu_lists_undo_redo() {
        let (mut app, dir) = test_app("menu-undo-items");
        app.open_menu = Some(crate::app::MenuKind::Edit);
        let items = app.menu_items();
        assert!(items.contains(&("Undo".to_string(), 'z')), "{items:?}");
        assert!(items.contains(&("Redo".to_string(), 'y')), "{items:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_menu_z_y_trigger_undo_redo() {
        let (mut app, dir) = test_app("menu-undo-act");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        std::fs::write(dir.join("Library").join("G").join("S.md"), "# S\n").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.open_selected_sheet();
        let end = app.editor.as_ref().unwrap().rope.len_chars();
        app.editor.as_mut().unwrap().cursor = end;
        app.ed_insert_char('x');
        // plain 'z' with the Edit menu open undoes, like Ctrl+Z does.
        app.open_menu = Some(crate::app::MenuKind::Edit);
        handle_key(&mut app, KeyCode::Char('z'), KeyModifiers::NONE);
        assert!(app.open_menu.is_none());
        assert!(app.editor.as_ref().unwrap().rope.to_string().ends_with("\n"));
        // uppercase 'Y' redoes.
        app.open_menu = Some(crate::app::MenuKind::Edit);
        handle_key(&mut app, KeyCode::Char('Y'), KeyModifiers::NONE);
        assert!(app.editor.as_ref().unwrap().rope.to_string().ends_with("x"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_empty_query_cancels() {
        let (mut app, dir) = test_app("find-empty");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.focus = Focus::Editor;
        app.open_selected_sheet();
        handle_key(&mut app, KeyCode::Char('g'), KeyModifiers::CONTROL);
        assert!(app.search_prompt.is_some());
        handle_key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.search_prompt.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ctrl_digits_jump_panes_from_editor() {
        let (mut app, dir) = test_app("digits");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.open_selected_sheet();
        app.focus = Focus::Editor;
        let before = app.editor.as_ref().map(|e| e.text());
        // Ctrl+1 focuses Library instead of typing "1"
        handle_key(&mut app, KeyCode::Char('1'), KeyModifiers::CONTROL);
        assert_eq!(app.focus, Focus::Library);
        assert_eq!(app.editor.as_ref().map(|e| e.text()), before);
        // Ctrl+3 jumps back to the editor
        handle_key(&mut app, KeyCode::Char('3'), KeyModifiers::CONTROL);
        assert_eq!(app.focus, Focus::Editor);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ctrl_digits_respect_hidden_panels() {
        let (mut app, dir) = test_app("digits-hidden");
        app.show_panels = false;
        app.focus = Focus::Editor;
        handle_key(&mut app, KeyCode::Char('1'), KeyModifiers::CONTROL);
        assert_eq!(app.focus, Focus::Editor);
        handle_key(&mut app, KeyCode::Char('2'), KeyModifiers::CONTROL);
        assert_eq!(app.focus, Focus::Editor);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn alt_digits_mirror_ctrl_digits() {
        let (mut app, dir) = test_app("digits-alt");
        crate::library::Library::create_group(&dir.join("Library"), std::path::Path::new(""), "G")
            .unwrap();
        crate::library::Library::create_sheet(&dir.join("Library"), "G", "S").unwrap();
        app.library = crate::library::Library::scan(&dir.join("Library"));
        app.open_selected_sheet();
        app.focus = Focus::Editor;
        let before = app.editor.as_ref().map(|e| e.text());
        // Alt+1 focuses Library instead of typing "1"
        handle_key(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);
        assert_eq!(app.focus, Focus::Library);
        assert_eq!(app.editor.as_ref().map(|e| e.text()), before);
        // Alt+2 focuses Sheets, Alt+3 returns to the editor
        handle_key(&mut app, KeyCode::Char('2'), KeyModifiers::ALT);
        assert_eq!(app.focus, Focus::Sheets);
        handle_key(&mut app, KeyCode::Char('3'), KeyModifiers::ALT);
        assert_eq!(app.focus, Focus::Editor);
        // hidden panels guard applies to Alt too
        app.show_panels = false;
        handle_key(&mut app, KeyCode::Char('1'), KeyModifiers::ALT);
        assert_eq!(app.focus, Focus::Editor);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
