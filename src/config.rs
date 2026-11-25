use std::path::{Path, PathBuf};

pub fn default_library_root() -> PathBuf {
    if let Ok(p) = std::env::var("ALCOTT_LIBRARY") {
        return PathBuf::from(p);
    }
    if let Some(home) = dirs::home_dir() {
        return home.join("Documents").join("Alcott").join("Library");
    }
    PathBuf::from("./Library")
}

pub fn resolve_library_root(cli_override: Option<PathBuf>) -> PathBuf {
    if let Some(p) = cli_override {
        return p;
    }
    default_library_root()
}

pub fn data_dir() -> PathBuf {
    if let Ok(p) = std::env::var("ALCOTT_DATA_DIR") {
        return PathBuf::from(p);
    }
    if let Some(d) = dirs::data_dir() {
        return d.join("alcott");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".local").join("share").join("alcott");
    }
    PathBuf::from("./.alcott")
}

pub fn ensure_dirs(library_root: &Path, data_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(library_root)?;
    std::fs::create_dir_all(data_dir)?;
    Ok(())
}

// Theme: terminal defaults only — no custom colors, so any terminal theme
// (light or dark) shines through. Hierarchy comes from text attributes.
pub mod theme {
    use ratatui::style::{Modifier, Style};
    /// Base style: terminal defaults.
    pub fn base() -> Style {
        Style::default()
    }
    /// De-emphasised text (paths, delimiters, metadata).
    pub fn dim() -> Style {
        Style::default().add_modifier(Modifier::DIM)
    }
    /// Emphasised text (headings, titles).
    pub fn bold() -> Style {
        Style::default().add_modifier(Modifier::BOLD)
    }
    /// Selected rows / focused items: the terminal's own reverse video.
    pub fn selected() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }
    /// Accelerator letter: the key to hold Ctrl with. Bold + underlined
    /// so it reads as "press Ctrl+this letter".
    pub fn accel() -> Style {
        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    pub fn accel_dim() -> Style {
        dim()
    }

    /// De-emphasised Markdown delimiters (`**`, backticks, brackets…).
    pub fn md_sym() -> Style {
        dim()
    }

    /// Highlighted block markers: `#` headings, list bullets, `>`.
    pub fn md_marker() -> Style {
        bold()
    }

    /// Inline `code` content: reverse video.
    pub fn md_code() -> Style {
        Style::default().add_modifier(Modifier::REVERSED)
    }

    /// Link text (patched over the surrounding style, so headings stay bold).
    pub fn md_link_text() -> Style {
        Style::default().add_modifier(Modifier::UNDERLINED)
    }
}
