use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "alcott", version, about = "Speed-focused local markdown writing app")]
pub struct Cli {
    /// Override library root (default ~/Documents/Alcott/Library or $ALCOTT_LIBRARY)
    #[arg(long, global = true)]
    pub library: Option<PathBuf>,

    /// Enable mouse support (click + wheel; on by default)
    #[arg(long, overrides_with = "no_mouse")]
    pub mouse: bool,

    /// Disable mouse support (terminal text selection works natively)
    #[arg(long = "no-mouse", overrides_with = "mouse")]
    pub no_mouse: bool,

    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Create a new sheet
    New {
        /// Sheet title (becomes Title.md)
        title: String,
        /// Group path, e.g. Writing/Book 1
        #[arg(short, long, default_value = "")]
        group: String,
    },
    /// List groups and/or sheets
    List {
        /// Only show this group
        #[arg(long)]
        group: Option<String>,
    },
    /// Print a sheet to stdout
    Print {
        /// Path, group-relative path, or fuzzy name
        target: String,
    },
    /// Search names + bodies (case-insensitive substring)
    Search {
        query: String,
        #[arg(short, long)]
        group: Option<String>,
    },
    /// Export sheets with a CSS style (like Ariadne: CSS drives the output)
    Export {
        /// Path, group-relative path, or fuzzy name (single sheet)
        target: Option<String>,
        /// Export a whole group instead of one sheet
        #[arg(long)]
        group: Option<String>,
        /// Output format: md, html, rtf, pdf, docx, epub
        #[arg(short, long, default_value = "html")]
        format: String,
        /// Style name: Built-in, Manuscript, Student, Paperback Novel, Professional, or a saved .css
        #[arg(short, long, default_value = "Built-in")]
        style: String,
        /// Use an outside .css file directly (also saved as active style text)
        #[arg(long)]
        css: Option<PathBuf>,
        /// Output file (defaults to <stem>.<ext> in cwd; adds " 2", " 3" on conflict)
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Overwrite the output file if it exists
        #[arg(long)]
        force: bool,
        /// Attach a font file (.ttf/.otf/.ttc) for export-only fonts (repeatable)
        #[arg(long)]
        font: Vec<PathBuf>,
    },
}
