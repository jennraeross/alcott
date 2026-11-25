# Alcott

Speed-focused local markdown writing app. Pure Rust TUI + CLI, filesystem as
database. Uses your terminal's own colors and theme — no custom colors.

![license: MIT](https://img.shields.io/badge/license-MIT-blue)

## Layout (matches `Frame 1.png`)

`Library (groups, arbitrary depth) | Sheets (name + preview) | Editor (raw markdown)` with top bar and bottom `path | saved | words | net · typed today`.

## Usage

```sh
alcott                          # TUI
alcott new "Title" -g Writing/"Book 1"
alcott list [--group Writing]
alcott print Writing/"Book 1"/Sheet3.md
alcott search "query" [-g Writing]
alcott export Sheet3 --format pdf --style Manuscript --out out.pdf [--force]
alcott --library /custom/root    # override ~/Documents/Alcott/Library
```

Library layout:

```
~/Documents/Alcott/Library/<any>/<depth>/*.md
```

Stats (`both` counters):

- `typed today` (gross): +1 on every completed word you type, even if later deleted.
- `net` (words added): `current_library_total - day_start_baseline`, goes down on deletions.

Stored in `<data_dir>/alcott/daily.json` (`$ALCOTT_DATA_DIR` overrides).

## Keys

Accelerator convention: the **bold + underlined letter** is the key to hold
`Ctrl` with. `^S` = `Ctrl+S`.

- `Ctrl+F` File menu (`N` new sheet, `G` new group, `S` save, `D` delete sheet, `Q` quit)
- `Ctrl+E` Edit menu (`E` focus editor, `K` delete line, `F` find in sheet, `Z` undo, `Y` redo)
- `Ctrl+G` find in open sheet (`Enter` next match, wraps; `Esc` closes)
- `Ctrl+V` View menu (`1/2/3` focus panes, `P` toggle panels, `M` markdown guide, `O` mouse on/off, `H` help)
- `Ctrl+X` Xport menu (`S` save, `P` preview, `O` open, `N` next style, `F` next format, `M` manage)
  dialog: `Tab` switches Style/Format, `j/k` picks, `Enter`/`S` saves, `U` safe-save
  (` 2` suffix), `T` output path, `N/E/I/D` new/edit-copy/import/delete style,
  `F` attach font, `C` clear fonts, existing files need a second `Enter` to overwrite
- `Ctrl+S` save, `Ctrl+Q` quit, `Ctrl+Z` undo, `Ctrl+Y` redo, `Ctrl+P` toggle side panels (also top-right as `^S save  ^Q quit  ^P panels`; `Ctrl+.` kept as fallback), `Ctrl+O` quick preview (same as Xport `P`: zathura for PDF/ePub, text overlay otherwise; `Esc` closes the overlay)
- `1/2/3` or `Tab`: focus Library / Sheets / Editor (skipped while panels hidden).
  `Alt+1/2/3` (or `Ctrl+1/2/3` where the terminal passes it through) works
  from anywhere, including while typing in the editor. Linux/Windows: Alt;
  macOS: Ctrl, or Alt with Option-as-Meta
- Library: `j/k` navigate, `h/l` collapse/expand, `space` toggle, `Enter` → Sheets, `N` new subgroup (sibling level), `R` rename, `D` delete (press again to confirm, recursive), `X` mark, `M` lift for move
- Sheets: `j/k` navigate, `Enter` open in Editor, `N` new sheet, `r` rename, `D` delete (press again to confirm), `X` mark, `A` mark all, `J`/`K` reorder, `M` lift for move
- Editor (always-insert): type, `Enter` (autocontinues `-`/`*`/`+`/`1.`/`1)`/`>`; numbers increment), `Backspace/Delete`, arrows, `Ctrl+Left` word jump, `Ctrl+K` delete line, `Esc` back (autosaves, 500ms debounce). The cursor is drawn as reverse-video on the character (theme-native, always visible); the terminal cursor is hidden while editing
- Marks & move: `X` toggles membership (`*` badge, count in statusbar), `M` lifts marked (or current) items — navigate, `Enter` drops into the highlighted group, `Esc` cancels. `D` deletes all marked (confirm shows the count)
- Multi-view: marking 2+ sheets shows them sequentially in the editor with `── path ──` dividers; every segment edits and saves to its own file. `Enter` on a sheet returns to single view
- Menus: `j/k` move, `Enter` run, accelerator letter runs directly, `Esc` closes
- Mouse (on by default): click pane to focus, click group to select, click
  sheet to open, click File/Edit/View for menus, click menu item to run,
  click elsewhere to close menu, wheel scrolls, drag pane separators to
  resize (12/30 col minimums, persisted). `Shift+drag` still selects
  text in most terminals. Toggle: View menu `O` or `--mouse` / `--no-mouse`
  (persisted)

## Editor preview

Inline Markdown is styled as you type: `**bold**`, `*italic*`, `__underline__`,
`` `code` `` and fenced blocks, `[text](url)` / `[text][ref]`, `# headings`
(bold), `-`/`1.` lists and `>` quotes. Markup symbols stay visible but dimmed
(accent for heading/list/fence markers), so columns never shift. Numbered
lists continue with the next number (`1.` → `2.`). Full table: View menu →
`M`arkdown guide.

## Export (CSS drives output, like Ariadne)

`Ctrl+X` (Xport menu) or `alcott export`. Pick a style, then a format:
Markdown, HTML, PDF, Word, Rich Text, ePub.

- Styles: `Built-in` (empty CSS) + Manuscript, Student, Paperback Novel,
  Professional, plus your own `*.css` in `<data_dir>/alcott/styles/`
  (`$ALCOTT_DATA_DIR` overrides). Same selectors as Ariadne:
  `body/p/li/ul/ol, h1/h2/h3, blockquote, pre/code, a` + plain `@page`.
  Supported: `color, font-size, font-weight/style, text-align, line-height,
  margin-bottom, text-indent, margin-left/right, break-before/after,
  font-family, @page { size, margin }`. HTML/ePub embed the raw CSS
  verbatim; PDF/Word/RTF map the subset above.
- Fonts: every built-in style stacks macOS/Windows names first, then
  Linux-prevalent fallbacks (`Liberation`, `DejaVu`, `Noto`). PDF embeds the
  first *installed* stack entry (attached fonts win, then system fonts, then
  generic); Word/RTF name it the same way. When the author's first choice is
  missing you get a warning on every export with install hints, e.g.
  `"Helvetica Neue" not installed, using "DejaVu Sans" instead
  (Debian/Ubuntu: apt install fonts-liberation2; Fedora: dnf install
  liberation-sans-fonts; Arch: pacman -S ttf-liberation; NixOS:
  fonts.packages += pkgs.liberation_ttf)`. Shown in the status bar, the
  preview panel, and CLI stderr; only styles the export actually uses warn.
- `N` new style (from template), `E` edit copy externally, `I` import `.css`,
  `D` delete, `F` attach export-only font (`.ttf`/`.otf`/`.ttc`, embedded in
  PDF via `krilla` + `fontdb`), `C` clear fonts. Choice + fonts persist in
  `<data_dir>/alcott/export.json`.
- Conflict handling (never blind-overwrites): existing output needs a second
  `Enter` to overwrite, or `U` writes `name 2.ext` instead; style names and
  CLI `--out` do the same ` 2`, ` 3` suffixing. CLI `--force` overwrites.
- Live preview (Linux): `P` on PDF/ePub launches `zathura --fork` on one
  stable tmp file per format (`/tmp/alcott-preview-<pid>.pdf/.epub`) instead
  of the in-TUI overlay; style/format changes rewrite it in place so the
  viewer reloads live. Missing zathura falls back to the text overlay.
  `Ctrl+O` from anywhere launches the same preview without opening the
  export menu (`Esc` closes the text overlay).

## Manual order

Sheets keep a manual order per group in `Library/.alcott/order.json`
(unlisted files append alphabetically; stale entries pruned on load).
`J`/`K` swaps with the neighbor, move-drop inserts before the highlighted
sheet. Groups stay alphabetical; moving a group to a new parent is the
group rearrangement.

## Speed

No async runtime, `ropey` buffers, lazy body loads, incremental word counts, release `lto + strip + codegen-units=1`.

## Layout

Side panels (Library + Sheets) toggle together with `Ctrl+P` (View menu `P` too).
Hidden editor centers at max 80 columns. Visibility persists in
`<data_dir>/alcott/state.json`. The statusbar shows pane controls when
Library/Sheets is focused (path only in the editor). While editing, both the
open sheet and its group show the reverse-block highlight, so the document's
location is always visible.

Statusbar path shrinks to fit: full → without `~/Documents/Alcott/Library` →
file only → middle-truncated title; stats shed `today`/typed/net clauses if
still squeezed. A ≥1-space gap is always kept.

## License

MIT — see `LICENSE-MIT`. `deny.toml` pins `cargo-deny` to MIT-compatible licenses only.
