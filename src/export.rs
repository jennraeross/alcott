//! CSS-driven export, ported from Ariadne.
//!
//! The stylesheet determines the output: the same selectors and properties
//! work for every format. HTML/ePub embed the raw CSS verbatim (full
//! fidelity); PDF/Word/RTF map a subset (color, size, weight, style,
//! alignment, line height, indents, breaks, family, `@page` size/margins).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Formats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExportFormat {
    Md,
    Html,
    Rtf,
    #[default]
    Pdf,
    Docx,
    Epub,
}

impl ExportFormat {
    #[allow(dead_code)]
    pub fn all() -> [ExportFormat; 6] {
        use ExportFormat::*;
        [Md, Html, Rtf, Pdf, Docx, Epub]
    }

    pub fn grid_order() -> [ExportFormat; 6] {
        use ExportFormat::*;
        [Pdf, Docx, Epub, Html, Rtf, Md]
    }

    pub fn label(&self) -> &'static str {
        match self {
            ExportFormat::Md => "Markdown (.md)",
            ExportFormat::Rtf => "Rich Text (.rtf)",
            ExportFormat::Pdf => "PDF (.pdf)",
            ExportFormat::Html => "Web page (.html)",
            ExportFormat::Docx => "Word (.docx)",
            ExportFormat::Epub => "ePub (.epub)",
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            ExportFormat::Pdf => "PDF",
            ExportFormat::Docx => "DOCX",
            ExportFormat::Epub => "Epub",
            ExportFormat::Html => "HTML",
            ExportFormat::Rtf => "RTF",
            ExportFormat::Md => "Markdown",
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Md => "md",
            ExportFormat::Rtf => "rtf",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Html => "html",
            ExportFormat::Docx => "docx",
            ExportFormat::Epub => "epub",
        }
    }

    pub fn from_extension(s: &str) -> Option<ExportFormat> {
        match s.to_lowercase().as_str() {
            "md" | "markdown" => Some(ExportFormat::Md),
            "html" | "htm" => Some(ExportFormat::Html),
            "rtf" => Some(ExportFormat::Rtf),
            "pdf" => Some(ExportFormat::Pdf),
            "docx" => Some(ExportFormat::Docx),
            "epub" => Some(ExportFormat::Epub),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Style constants (same five as Ariadne)
// ---------------------------------------------------------------------------

pub const STYLE_TEMPLATE: &str = "/* My style — describe it here. */\nbody {\n  color: #222233;\n  font-size: 14px;\n  line-height: 1.6;\n}\nh1 {\n  color: #b00020;\n}\na {\n  color: #0066cc;\n}\n@page {\n  size: A4;\n  margin: 2cm;\n}\n";

pub const PROFESSIONAL_STYLE: &str = "/* Professional — a clean, neutral theme for exports. */\n/* Only color, font size/weight/style, alignment, line height, */\n/* indents, page breaks, @page size and margins map onto PDF, Word */\n/* and RTF; everything else applies to HTML and ePub. */\nbody {\n  font-family: \"Helvetica Neue\", Helvetica, Arial, \"Liberation Sans\", \"DejaVu Sans\", \"Noto Sans\", sans-serif;\n  color: #1f2933;\n  font-size: 11pt;\n  line-height: 1.65;\n  margin-bottom: 6pt;\n}\nh1 {\n  font-size: 24pt;\n  font-weight: bold;\n  color: #0f2a44;\n  margin-bottom: 12pt;\n}\nh2 {\n  font-size: 16pt;\n  font-weight: bold;\n  color: #1b3a5c;\n  margin-bottom: 8pt;\n}\nh3 {\n  font-size: 12.5pt;\n  font-weight: bold;\n  color: #1b3a5c;\n}\na {\n  color: #0066cc;\n}\nblockquote {\n  color: #52616f;\n  font-style: italic;\n  margin-left: 18pt;\n}\npre {\n  font-family: \"SF Mono\", Menlo, Consolas, \"Courier New\", Courier, \"Liberation Mono\", \"DejaVu Sans Mono\", \"Noto Sans Mono\", monospace;\n  font-size: 9.5pt;\n}\n@page {\n  size: A4;\n  margin: 2.5cm 2cm;\n}\n";

pub const MANUSCRIPT_STYLE: &str = "/* Manuscript — standard submission format. */\nbody {\n  font-family: \"Courier New\", Courier, \"Liberation Mono\", \"DejaVu Sans Mono\", \"Noto Sans Mono\", monospace;\n  color: #000000;\n  font-size: 11pt;\n  line-height: 2;\n  text-indent: 36pt;\n  margin-bottom: 0;\n}\nh1 {\n  font-size: 11pt;\n  font-weight: normal;\n  text-align: center;\n  text-indent: 0;\n  margin-bottom: 12pt;\n  break-before: always;\n}\nh2 {\n  font-size: 11pt;\n  font-weight: normal;\n  text-align: center;\n  text-indent: 0;\n  margin-bottom: 12pt;\n}\nh3 {\n  font-size: 11pt;\n  font-weight: normal;\n  text-align: center;\n  text-indent: 0;\n  margin-bottom: 12pt;\n}\na {\n  color: #000000;\n}\nblockquote {\n  margin-left: 36pt;\n  margin-right: 36pt;\n  text-indent: 0;\n}\npre {\n  font-size: 11pt;\n  text-indent: 0;\n}\n@page {\n  size: letter;\n  margin: 1in;\n}\n";

pub const STUDENT_STYLE: &str = "/* Student — MLA-style paper. */\nbody {\n  font-family: \"Times New Roman\", Times, \"Liberation Serif\", \"DejaVu Serif\", \"Noto Serif\", serif;\n  color: #000000;\n  font-size: 12pt;\n  line-height: 2;\n  text-indent: 36pt;\n  margin-bottom: 0;\n}\nh1 {\n  font-size: 12pt;\n  font-weight: normal;\n  text-align: center;\n  text-indent: 0;\n  margin-bottom: 12pt;\n}\nh2 {\n  font-size: 12pt;\n  font-weight: bold;\n  text-align: left;\n  text-indent: 0;\n  margin-bottom: 12pt;\n}\nh3 {\n  font-size: 12pt;\n  font-weight: bold;\n  text-align: left;\n  text-indent: 0;\n  margin-bottom: 12pt;\n}\na {\n  color: #000000;\n}\nblockquote {\n  margin-left: 36pt;\n  text-indent: 0;\n  line-height: 2;\n}\npre {\n  font-size: 12pt;\n  text-indent: 0;\n}\n@page {\n  size: letter;\n  margin: 1in;\n}\n";

pub const PAPERBACK_STYLE: &str = "/* Paperback Novel — justified serif pages. */\nbody {\n  font-family: Georgia, \"Times New Roman\", \"Liberation Serif\", \"DejaVu Serif\", \"Noto Serif\", serif;\n  color: #1a1a1a;\n  font-size: 11pt;\n  line-height: 1.5;\n  text-align: justify;\n  text-indent: 18pt;\n  margin-bottom: 0;\n}\nh1 {\n  font-size: 16pt;\n  font-weight: normal;\n  text-align: center;\n  text-indent: 0;\n  margin-bottom: 18pt;\n  break-before: always;\n}\nh2 {\n  font-size: 13pt;\n  font-weight: bold;\n  text-align: center;\n  text-indent: 0;\n  margin-bottom: 12pt;\n}\nh3 {\n  font-size: 11pt;\n  font-weight: bold;\n  text-align: center;\n  text-indent: 0;\n  margin-bottom: 12pt;\n}\na {\n  color: #1a1a1a;\n}\nblockquote {\n  font-style: italic;\n  margin-left: 18pt;\n  margin-right: 18pt;\n  text-indent: 0;\n}\npre {\n  font-family: \"Courier New\", Courier, \"Liberation Mono\", \"DejaVu Sans Mono\", \"Noto Sans Mono\", monospace;\n  font-size: 9.5pt;\n  text-indent: 0;\n}\n@page {\n  size: 5.5in 8.5in;\n  margin: 0.75in;\n}\n";

pub const VIRTUAL_STYLES: [(&str, &str); 4] = [
    ("Manuscript", MANUSCRIPT_STYLE),
    ("Student", STUDENT_STYLE),
    ("Paperback Novel", PAPERBACK_STYLE),
    ("Professional", PROFESSIONAL_STYLE),
];

pub fn virtual_style_css(name: &str) -> Option<&'static str> {
    VIRTUAL_STYLES.iter().find(|(n, _)| *n == name).map(|(_, css)| *css)
}

pub fn is_reserved_style(name: &str) -> bool {
    name == "Built-in" || virtual_style_css(name).is_some()
}

/// Default code stack reused by the built-in theme before any CSS applies.
const MONO_STACK: [&str; 6] = [
    "Courier New",
    "Courier",
    "Liberation Mono",
    "DejaVu Sans Mono",
    "Noto Sans Mono",
    "monospace",
];

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Clone)]
pub struct ElStyle {
    pub size_pt: f32,
    pub color: Option<(u8, u8, u8)>,
    pub bold: bool,
    pub italic: bool,
    pub align: ExportAlign,
    pub line_height: Option<f32>,
    pub space_after_pt: Option<f32>,
    /// First family in the CSS stack (drives the PDF width estimate).
    /// HTML/ePub use the raw stack verbatim; PDF/Word/RTF resolve through
    /// `family_stack` so widely-installed Linux fonts are picked up.
    pub family: Option<String>,
    /// Every family in the CSS stack, in author order.
    pub family_stack: Vec<String>,
    pub text_indent: Option<f32>,
    pub margin_left: Option<f32>,
    pub margin_right: Option<f32>,
    pub break_before: bool,
    pub break_after: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PdfFamily {
    Sans,
    Serif,
    Mono,
}

pub fn pdf_family_for(name: &str) -> PdfFamily {
    let n = name.trim().to_lowercase();
    if n.contains("mono") || n.contains("courier") || n.contains("consola") || n.contains("menlo") || n == "monospace" {
        PdfFamily::Mono
    } else if n.contains("sans") || n.contains("arial") || n.contains("helvetica") || n.contains("verdana") || n.contains("trebuchet") || n.contains("tahoma") || n.contains("system") {
        PdfFamily::Sans
    } else if n.contains("serif") || n.contains("georgia") || n.contains("times") || n.contains("garamond") || n.contains("palatino") || n.contains("cambria") {
        PdfFamily::Serif
    } else {
        PdfFamily::Sans
    }
}

impl ElStyle {
    fn plain(size_pt: f32) -> Self {
        Self {
            size_pt,
            color: None,
            bold: false,
            italic: false,
            align: ExportAlign::Left,
            line_height: None,
            space_after_pt: None,
            family: None,
            family_stack: Vec::new(),
            text_indent: None,
            margin_left: None,
            margin_right: None,
            break_before: false,
            break_after: false,
        }
    }

    pub fn pdf_family(&self) -> PdfFamily {
        self.family.as_deref().map(pdf_family_for).unwrap_or(PdfFamily::Sans)
    }
}

#[derive(Debug, Clone, Default)]
pub struct PageStyle {
    pub size: Option<(f32, f32)>,
    pub margin: [Option<f32>; 4],
}

#[derive(Debug, Clone)]
pub struct ExportTheme {
    pub title: ElStyle,
    pub h1: ElStyle,
    pub h2: ElStyle,
    pub h3: ElStyle,
    pub body: ElStyle,
    pub quote: ElStyle,
    pub code: ElStyle,
    pub link_color: Option<(u8, u8, u8)>,
    pub page: Option<PageStyle>,
    pub raw_css: String,
}

impl ExportTheme {
    pub fn builtin() -> Self {
        let mut h1 = ElStyle::plain(16.0);
        h1.bold = true;
        let mut h2 = ElStyle::plain(14.0);
        h2.bold = true;
        let mut h3 = ElStyle::plain(12.5);
        h3.bold = true;
        let mut title = ElStyle::plain(18.0);
        title.bold = true;
        let mut quote = ElStyle::plain(11.0);
        quote.italic = true;
        let mut code = ElStyle::plain(10.0);
        code.family = Some("Courier New".to_string());
        code.family_stack = MONO_STACK.iter().map(|s| s.to_string()).collect();
        Self {
            title, h1, h2, h3,
            body: ElStyle::plain(11.0),
            quote, code,
            link_color: None,
            page: None,
            raw_css: String::new(),
        }
    }

    pub fn from_css(css: &str) -> Self {
        let mut theme = Self::builtin();
        theme.raw_css = css.to_string();
        for (selector, decls) in Self::parse_rules(css) {
            for part in selector.split(',') {
                let sel = part.trim();
                if sel == "h1" {
                    let base = theme.body.size_pt;
                    Self::apply_decls(&mut theme.h1, &decls, base);
                    let base = theme.body.size_pt;
                    Self::apply_decls(&mut theme.title, &decls, base);
                    continue;
                }
                if sel == "a" {
                    for (prop, val) in &decls {
                        if prop == "color" {
                            if let Some(c) = Self::parse_color(val) {
                                theme.link_color = Some(c);
                            }
                        }
                    }
                    continue;
                }
                let base = theme.body.size_pt;
                let slot: &mut ElStyle = match sel {
                    "h2" => &mut theme.h2,
                    "h3" => &mut theme.h3,
                    "blockquote" => &mut theme.quote,
                    "pre" | "code" => &mut theme.code,
                    "body" | "p" | "li" | "ul" | "ol" => &mut theme.body,
                    _ => continue,
                };
                Self::apply_decls(slot, &decls, base);
            }
        }
        for el in [&mut theme.title, &mut theme.h1, &mut theme.h2, &mut theme.h3, &mut theme.quote, &mut theme.code] {
            if el.color.is_none() {
                el.color = theme.body.color;
            }
            if el.text_indent.is_none() {
                el.text_indent = theme.body.text_indent;
            }
            if el.margin_left.is_none() {
                el.margin_left = theme.body.margin_left;
            }
            if el.margin_right.is_none() {
                el.margin_right = theme.body.margin_right;
            }
        }
        for el in [&mut theme.title, &mut theme.h1, &mut theme.h2, &mut theme.h3, &mut theme.quote] {
            if el.family.is_none() {
                el.family = theme.body.family.clone();
            }
            if el.family_stack.is_empty() {
                el.family_stack = theme.body.family_stack.clone();
            }
        }
        for (selector, decls) in Self::parse_page_blocks(css) {
            if selector.is_empty() {
                let mut page = theme.page.unwrap_or_default();
                Self::apply_page_decls(&mut page, &decls);
                theme.page = Some(page);
            }
        }
        theme
    }

    pub fn page_dims(&self) -> (f32, f32) {
        self.page.as_ref().and_then(|p| p.size).unwrap_or((595.0, 842.0))
    }

    pub fn page_margin(&self, side: usize) -> Option<f32> {
        self.page.as_ref().and_then(|p| p.margin.get(side).copied().flatten())
    }

    fn apply_page_decls(style: &mut PageStyle, decls: &[(String, String)]) {
        for (prop, val) in decls {
            match prop.as_str() {
                "size" => {
                    if let Some(size) = Self::parse_page_size(val) {
                        style.size = Some(size);
                    }
                }
                "margin" => {
                    if let Some(m) = Self::parse_margin_shorthand(val) {
                        style.margin = m.map(Some);
                    }
                }
                "margin-top" => {
                    if let Some(pt) = Self::parse_page_length(val) {
                        style.margin[0] = Some(pt);
                    }
                }
                "margin-right" => {
                    if let Some(pt) = Self::parse_page_length(val) {
                        style.margin[1] = Some(pt);
                    }
                }
                "margin-bottom" => {
                    if let Some(pt) = Self::parse_page_length(val) {
                        style.margin[2] = Some(pt);
                    }
                }
                "margin-left" => {
                    if let Some(pt) = Self::parse_page_length(val) {
                        style.margin[3] = Some(pt);
                    }
                }
                _ => {}
            }
        }
    }

    fn apply_decls(el: &mut ElStyle, decls: &[(String, String)], base_pt: f32) {
        for (prop, val) in decls {
            match prop.as_str() {
                "color" => {
                    if let Some(c) = Self::parse_color(val) {
                        el.color = Some(c);
                    }
                }
                "font-size" => {
                    if let Some(pt) = Self::parse_size_pt(val, base_pt.max(el.size_pt)) {
                        el.size_pt = pt;
                    }
                }
                "font-weight" => {
                    let lowered = val.to_lowercase();
                    let v: &str = &lowered;
                    el.bold = matches!(v, "bold" | "bolder" | "700" | "800" | "900")
                        || v.parse::<u32>().map(|n| n >= 700).unwrap_or(false);
                    if v == "normal" {
                        el.bold = false;
                    }
                }
                "font-style" => {
                    let v = val.to_lowercase();
                    el.italic = v == "italic" || v == "oblique";
                }
                "text-align" => {
                    el.align = match val.to_lowercase().as_str() {
                        "center" => ExportAlign::Center,
                        "right" => ExportAlign::Right,
                        "justify" => ExportAlign::Justify,
                        _ => ExportAlign::Left,
                    };
                }
                "line-height" => {
                    if let Some(m) = Self::parse_line_height(val, el.size_pt) {
                        el.line_height = Some(m);
                    }
                }
                "margin-bottom" => {
                    if let Some(pt) = Self::parse_size_pt(val, el.size_pt) {
                        el.space_after_pt = Some(pt);
                    }
                }
                "text-indent" => {
                    if let Some(pt) = Self::parse_element_length(val, el.size_pt) {
                        el.text_indent = Some(pt);
                    }
                }
                "margin-left" => {
                    if let Some(pt) = Self::parse_element_length(val, el.size_pt) {
                        el.margin_left = Some(pt);
                    }
                }
                "margin-right" => {
                    if let Some(pt) = Self::parse_element_length(val, el.size_pt) {
                        el.margin_right = Some(pt);
                    }
                }
                "break-before" | "page-break-before" => {
                    el.break_before = matches!(val.trim().to_lowercase().as_str(), "always" | "page");
                }
                "break-after" | "page-break-after" => {
                    el.break_after = matches!(val.trim().to_lowercase().as_str(), "always" | "page");
                }
                "font-family" => {
                    let stack = Self::parse_font_stack(val);
                    el.family = stack.first().cloned();
                    el.family_stack = stack;
                }
                _ => {}
            }
        }
    }

    fn parse_rules(css: &str) -> Vec<(String, Vec<(String, String)>)> {
        let mut clean = String::with_capacity(css.len());
        let mut chars = css.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                let mut prev_star = false;
                while let Some(c2) = chars.next() {
                    if prev_star && c2 == '/' {
                        break;
                    }
                    prev_star = c2 == '*';
                }
            } else {
                clean.push(c);
            }
        }
        let mut rules = Vec::new();
        for chunk in clean.split('}') {
            let Some((sel, decls)) = chunk.split_once('{') else {
                continue;
            };
            let selector = sel.trim().to_lowercase();
            if selector.is_empty() || selector.starts_with('@') {
                continue;
            }
            let parsed = Self::parse_decls(decls);
            if !parsed.is_empty() {
                rules.push((selector, parsed));
            }
        }
        rules
    }

    fn parse_decls(block: &str) -> Vec<(String, String)> {
        let mut parsed = Vec::new();
        for decl in block.split(';') {
            let Some((prop, val)) = decl.split_once(':') else {
                continue;
            };
            let prop = prop.trim().to_lowercase();
            let val = val.trim().to_string();
            if !prop.is_empty() && !val.is_empty() {
                parsed.push((prop, val));
            }
        }
        parsed
    }

    /// Split a CSS `font-family` stack into names in author order, dropping
    /// quotes, empties, generic keywords handled by callers, and CSS-wide
    /// keywords (`inherit`/`initial`/`unset`) which mean "no opinion".
    fn parse_font_stack(val: &str) -> Vec<String> {
        let mut out = Vec::new();
        for part in val.split(',') {
            let first = part.trim();
            let name = first
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .or_else(|| first.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
                .unwrap_or(first)
                .trim();
            match name.to_lowercase().as_str() {
                "" | "inherit" | "initial" | "unset" => {}
                _ => out.push(name.to_string()),
            }
        }
        out
    }

    fn parse_page_blocks(css: &str) -> Vec<(String, Vec<(String, String)>)> {
        let b = css.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            let is_at = b[i] == b'@'
                && css.get(i..i + 5).is_some_and(|s| s.eq_ignore_ascii_case("@page"))
                && b.get(i + 5).is_none_or(|c| !(c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_'));
            if !is_at {
                i += 1;
                continue;
            }
            let mut j = i + 5;
            while j < b.len() && (b[j] as char).is_whitespace() {
                j += 1;
            }
            let sel_start = j;
            while j < b.len() && b[j] != b'{' {
                j += 1;
            }
            if j >= b.len() {
                break;
            }
            let selector = css.get(sel_start..j).unwrap_or("").trim().to_lowercase();
            let mut depth = 0usize;
            let mut k = j;
            while k < b.len() {
                if b[k] == b'{' {
                    depth += 1;
                } else if b[k] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                k += 1;
            }
            if k >= b.len() {
                break;
            }
            let body = css.get(j + 1..k).unwrap_or("");
            let decls = Self::parse_decls(&Self::strip_nested_at_blocks(body));
            out.push((selector, decls));
            i = k + 1;
        }
        out
    }

    fn strip_nested_at_blocks(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'@' {
                let mut j = i;
                while j < b.len() && b[j] != b'{' && b[j] != b';' {
                    j += 1;
                }
                if j < b.len() && b[j] == b'{' {
                    let mut depth = 0usize;
                    let mut k = j;
                    while k < b.len() {
                        if b[k] == b'{' {
                            depth += 1;
                        } else if b[k] == b'}' {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        k += 1;
                    }
                    i = (k + 1).min(b.len());
                } else {
                    while j < b.len() && b[j] != b';' {
                        j += 1;
                    }
                    i = (j + 1).min(b.len());
                }
            } else {
                let ch = s[i..].chars().next().unwrap();
                out.push(ch);
                i += ch.len_utf8();
            }
        }
        out
    }

    fn parse_page_length(val: &str) -> Option<f32> {
        let v = val.trim().to_lowercase();
        if let Some(px) = v.strip_suffix("px") {
            return px.trim().parse::<f32>().ok().map(|n| n * 0.75);
        }
        if let Some(pt) = v.strip_suffix("pt") {
            return pt.trim().parse::<f32>().ok();
        }
        if let Some(n) = v.strip_suffix("in") {
            return n.trim().parse::<f32>().ok().map(|n| n * 72.0);
        }
        if let Some(n) = v.strip_suffix("cm") {
            return n.trim().parse::<f32>().ok().map(|n| n * 28.3465);
        }
        if let Some(n) = v.strip_suffix("mm") {
            return n.trim().parse::<f32>().ok().map(|n| n * 2.83465);
        }
        if let Some(n) = v.strip_suffix('q') {
            return n.trim().parse::<f32>().ok().map(|n| n * 0.70866);
        }
        v.parse::<f32>().ok()
    }

    fn parse_margin_shorthand(val: &str) -> Option<[f32; 4]> {
        let parts: Vec<f32> = val.split_whitespace().map(Self::parse_page_length).collect::<Option<Vec<_>>>()?;
        match parts.as_slice() {
            [a] => Some([*a, *a, *a, *a]),
            [v, h] => Some([*v, *h, *v, *h]),
            [t, h, b] => Some([*t, *h, *b, *h]),
            [t, r, b, l] => Some([*t, *r, *b, *l]),
            _ => None,
        }
    }

    fn parse_page_size(val: &str) -> Option<(f32, f32)> {
        fn named(name: &str) -> Option<(f32, f32)> {
            Some(match name {
                "a5" => (419.53, 595.28),
                "a4" => (595.28, 841.89),
                "a3" => (841.89, 1190.55),
                "b5" => (498.90, 708.66),
                "b4" => (708.66, 1000.63),
                "letter" => (612.0, 792.0),
                "legal" => (612.0, 1008.0),
                "ledger" => (1224.0, 792.0),
                _ => return None,
            })
        }
        let mut lengths = Vec::new();
        let mut size = None;
        let mut landscape = false;
        for token in val.trim().to_lowercase().split_whitespace() {
            match token {
                "portrait" => {}
                "landscape" => landscape = true,
                "auto" => return None,
                t => {
                    if let Some(dim) = named(t) {
                        size = Some(dim);
                    } else if let Some(pt) = Self::parse_page_length(t) {
                        lengths.push(pt);
                    } else {
                        return None;
                    }
                }
            }
        }
        let mut dims = match (size, lengths.as_slice()) {
            (_, [w, h]) => (*w, *h),
            (_, [s]) => (*s, *s),
            (Some(d), []) => d,
            _ => return None,
        };
        if landscape {
            std::mem::swap(&mut dims.0, &mut dims.1);
        }
        Some(dims)
    }

    fn parse_color(val: &str) -> Option<(u8, u8, u8)> {
        let lowered = val.trim().to_lowercase();
        let v = lowered.as_str();
        if let Some(hex) = v.strip_prefix('#') {
            let digits: Vec<char> = hex.chars().collect();
            let expand = |c: char| c.to_digit(16).map(|d| (d * 17) as u8);
            return match digits.as_slice() {
                [r, g, b] => Some((expand(*r)?, expand(*g)?, expand(*b)?)),
                [r, g, b, _] => Some((expand(*r)?, expand(*g)?, expand(*b)?)),
                [r1, r2, g1, g2, b1, b2] => {
                    let n = |a: char, b: char| Some((a.to_digit(16)? * 16 + b.to_digit(16)?) as u8);
                    Some((n(*r1, *r2)?, n(*g1, *g2)?, n(*b1, *b2)?))
                }
                [r1, r2, g1, g2, b1, b2, _, _] => {
                    let n = |a: char, b: char| Some((a.to_digit(16)? * 16 + b.to_digit(16)?) as u8);
                    Some((n(*r1, *r2)?, n(*g1, *g2)?, n(*b1, *b2)?))
                }
                _ => None,
            };
        }
        if let Some(inner) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")).and_then(|s| s.strip_suffix(')')) {
            let parts: Vec<&str> = inner.split(',').collect();
            if parts.len() >= 3 {
                let num = |s: &str| {
                    let s = s.trim();
                    if let Some(pct) = s.strip_suffix('%') {
                        pct.trim().parse::<f32>().ok().map(|n| (n * 2.55) as u8)
                    } else {
                        s.parse::<u8>().ok()
                    }
                };
                return Some((num(parts[0])?, num(parts[1])?, num(parts[2])?));
            }
            return None;
        }
        Some(match v {
            "black" => (0, 0, 0),
            "white" => (255, 255, 255),
            "red" => (255, 0, 0),
            "green" => (0, 128, 0),
            "blue" => (0, 0, 255),
            "yellow" => (255, 255, 0),
            "cyan" | "aqua" => (0, 255, 255),
            "magenta" | "fuchsia" => (255, 0, 255),
            "gray" | "grey" => (128, 128, 128),
            "darkgray" | "darkgrey" => (169, 169, 169),
            "lightgray" | "lightgrey" => (211, 211, 211),
            "orange" => (255, 165, 0),
            "purple" => (128, 0, 128),
            "brown" => (165, 42, 42),
            "navy" => (0, 0, 128),
            "teal" => (0, 128, 128),
            "olive" => (128, 128, 0),
            "maroon" => (128, 0, 0),
            _ => return None,
        })
    }

    fn parse_element_length(val: &str, base_pt: f32) -> Option<f32> {
        if let Some(pt) = Self::parse_page_length(val) {
            return Some(pt);
        }
        let v = val.trim().to_lowercase();
        if let Some(em) = v.strip_suffix("rem") {
            return em.trim().parse::<f32>().ok().map(|n| n * base_pt);
        }
        if let Some(em) = v.strip_suffix("em") {
            return em.trim().parse::<f32>().ok().map(|n| n * base_pt);
        }
        None
    }

    fn parse_size_pt(val: &str, base_pt: f32) -> Option<f32> {
        let lowered = val.trim().to_lowercase();
        let v = lowered.as_str();
        if let Some(px) = v.strip_suffix("px") {
            return px.trim().parse::<f32>().ok().map(|n| n * 0.75);
        }
        if let Some(pt) = v.strip_suffix("pt") {
            return pt.trim().parse::<f32>().ok();
        }
        if let Some(em) = v.strip_suffix("rem") {
            return em.trim().parse::<f32>().ok().map(|n| n * base_pt);
        }
        if let Some(em) = v.strip_suffix("em") {
            return em.trim().parse::<f32>().ok().map(|n| n * base_pt);
        }
        if let Some(pct) = v.strip_suffix('%') {
            return pct.trim().parse::<f32>().ok().map(|n| base_pt * n / 100.0);
        }
        v.parse::<f32>().ok()
    }

    fn parse_line_height(val: &str, size_pt: f32) -> Option<f32> {
        let lowered = val.trim().to_lowercase();
        let v = lowered.as_str();
        if v == "normal" {
            return None;
        }
        if let Some(px) = v.strip_suffix("px") {
            return px.trim().parse::<f32>().ok().map(|n| n * 0.75 / size_pt.max(0.1));
        }
        if let Some(pt) = v.strip_suffix("pt") {
            return pt.trim().parse::<f32>().ok().map(|n| n / size_pt.max(0.1));
        }
        v.parse::<f32>().ok()
    }
}

// ---------------------------------------------------------------------------
// Markdown model (same block grammar as Ariadne)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Block {
    Heading(u8, String),
    Bullet(String),
    Numbered(usize, String),
    Task(bool, String),
    Quote(String),
    Code(String),
    Hr,
    Para(String),
    Image { alt: String, src: String },
}

#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub href: Option<String>,
}

/// One document to export: rendered content plus the directory its relative
/// image paths resolve against.
#[derive(Debug, Clone)]
pub struct SheetDoc {
    #[allow(dead_code)]
    pub title_hint: String,
    pub content: String,
    pub base_dir: PathBuf,
}

impl SheetDoc {
    pub fn new(title_hint: String, content: String, base_dir: PathBuf) -> Self {
        Self { title_hint, content, base_dir }
    }
}

pub fn parse_blocks(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut in_code = false;
    let mut code_buf = String::new();
    for raw in text.lines() {
        if raw.trim_start().starts_with("```") {
            if in_code {
                blocks.push(Block::Code(std::mem::take(&mut code_buf)));
                in_code = false;
            } else {
                in_code = true;
                code_buf.clear();
            }
            continue;
        }
        if in_code {
            code_buf.push_str(raw);
            code_buf.push('\n');
            continue;
        }
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lst = raw.trim_start();
        let mut level = 0u8;
        for prefix in ["### ", "## ", "# "] {
            if lst.starts_with(prefix) {
                level = prefix.trim().len() as u8;
                break;
            }
        }
        if level > 0 {
            blocks.push(Block::Heading(level, lst[level as usize + 1..].to_string()));
            continue;
        }
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            blocks.push(Block::Hr);
            continue;
        }
        if let Some(rest) = lst.strip_prefix("- [ ] ") {
            blocks.push(Block::Task(false, rest.to_string()));
            continue;
        }
        if let Some(rest) = lst.strip_prefix("- [x] ").or_else(|| lst.strip_prefix("- [X] ")) {
            blocks.push(Block::Task(true, rest.to_string()));
            continue;
        }
        if let Some(rest) = lst.strip_prefix("- ").or_else(|| lst.strip_prefix("* ")) {
            blocks.push(Block::Bullet(rest.to_string()));
            continue;
        }
        let mut digits = 0;
        for c in lst.chars() {
            if c.is_ascii_digit() {
                digits += c.len_utf8();
            } else {
                break;
            }
        }
        if digits > 0 && lst[digits..].starts_with(". ") {
            let n: usize = lst[..digits].parse().unwrap_or(1);
            blocks.push(Block::Numbered(n, lst[digits + 2..].to_string()));
            continue;
        }
        if let Some(rest) = lst.strip_prefix("> ") {
            blocks.push(Block::Quote(rest.to_string()));
            continue;
        }
        if let Some(rest) = lst.strip_prefix('>') {
            blocks.push(Block::Quote(rest.trim_start().to_string()));
            continue;
        }
        if let Some((alt, src)) = parse_image_ref(trimmed) {
            blocks.push(Block::Image { alt, src });
            continue;
        }
        blocks.push(Block::Para(trimmed.to_string()));
    }
    if in_code {
        blocks.push(Block::Code(code_buf));
    }
    blocks
}

pub fn parse_image_ref(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("![")?;
    let close = rest.find(']')?;
    if !rest[close..].starts_with("](") || !rest.ends_with(')') {
        return None;
    }
    let alt = rest[..close].to_string();
    let src = rest[close + 2..rest.len() - 1].trim().to_string();
    if src.is_empty() {
        None
    } else {
        Some((alt, src))
    }
}

pub fn is_image_line(line: &str) -> bool {
    parse_image_ref(line.trim()).is_some()
}

pub fn heading_plain_text(text: &str) -> String {
    text.replace("**", "").replace("__", "").replace('*', "").trim().to_string()
}

pub fn inferred_title(content: &str) -> String {
    for line in content.lines() {
        let mut t = line.trim();
        if t.is_empty() || is_image_line(t) {
            continue;
        }
        for prefix in ["# ", "## ", "### ", "> ", "- [ ] ", "- [x] ", "- [X] ", "- ", "* "] {
            if let Some(rest) = t.strip_prefix(prefix) {
                t = rest.trim_start();
                break;
            }
        }
        let clean = t.replace("**", "").replace("__", "").replace('*', "").trim().to_string();
        if clean.is_empty() {
            continue;
        }
        if clean.chars().count() > 60 {
            let trunc: String = clean.chars().take(57).collect();
            if let Some(pos) = trunc.rfind(' ') {
                return format!("{}…", &trunc[..pos]);
            }
            return format!("{trunc}…");
        }
        return clean;
    }
    "Untitled".to_string()
}

pub fn export_blocks(content: &str) -> Vec<Block> {
    let blocks = parse_blocks(content);
    let title = inferred_title(content);
    match blocks.first() {
        Some(Block::Heading(1, t)) if heading_plain_text(t) == title => blocks.into_iter().skip(1).collect(),
        _ => blocks,
    }
}

pub fn parse_spans(s: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut buf = String::new();
    let mut bold = false;
    let mut italic = false;
    let mut code = false;
    let flush = |buf: &mut String, spans: &mut Vec<Span>, bold: bool, italic: bool, code: bool| {
        if !buf.is_empty() {
            spans.push(Span { text: std::mem::take(buf), bold, italic, code, href: None });
        }
    };
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if !code && s[i..].starts_with('[') {
            if let Some(close) = s[i..].find("](") {
                if let Some(end) = s[i + close + 2..].find(')') {
                    let text = &s[i + 1..i + close];
                    let url = &s[i + close + 2..i + close + 2 + end];
                    flush(&mut buf, &mut spans, bold, italic, code);
                    spans.push(Span { text: text.to_string(), bold, italic, code, href: Some(url.to_string()) });
                    i += close + 3 + end;
                    continue;
                }
            }
            buf.push('[');
            i += 1;
            continue;
        }
        if s[i..].starts_with("**") {
            flush(&mut buf, &mut spans, bold, italic, code);
            bold = !bold;
            i += 2;
            continue;
        }
        if s[i..].starts_with("__") && !code {
            flush(&mut buf, &mut spans, bold, italic, code);
            bold = !bold;
            i += 2;
            continue;
        }
        if bytes[i] == b'`' {
            flush(&mut buf, &mut spans, bold, italic, code);
            code = !code;
            i += 1;
            continue;
        }
        if bytes[i] == b'*' && !code {
            flush(&mut buf, &mut spans, bold, italic, code);
            italic = !italic;
            i += 1;
            continue;
        }
        let c = s[i..].chars().next().unwrap();
        buf.push(c);
        i += c.len_utf8();
    }
    flush(&mut buf, &mut spans, bold, italic, code);
    spans
}

pub fn spans_plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct ImageStore {
    pub items: Vec<ExportImage>,
    pub by_src: HashMap<String, usize>,
}

pub struct ExportImage {
    pub name: String,
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub data_uri: String,
}

impl ImageStore {
    fn mime_for(ext: &str) -> &'static str {
        match ext {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            _ => "application/octet-stream",
        }
    }

    pub fn add(&mut self, src: &str, bytes: Vec<u8>, ext: &str) -> usize {
        if let Some(&i) = self.by_src.get(src) {
            return i;
        }
        let mime = Self::mime_for(ext);
        use base64::Engine as _;
        let data_uri = format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&bytes));
        let i = self.items.len();
        let name_ext = if ext == "jpeg" { "jpg" } else { ext };
        self.items.push(ExportImage { name: format!("img{i}.{name_ext}"), bytes, mime, data_uri });
        self.by_src.insert(src.to_string(), i);
        i
    }
}

pub fn sniff_image_kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else {
        None
    }
}

/// Resolve an image `src` relative to `base_dir`. Supports `data:` URIs,
/// `file://`, absolute paths and paths relative to the sheet directory.
/// Remote URLs are never fetched.
pub fn resolve_image_bytes(base_dir: &Path, src: &str) -> Option<(Vec<u8>, String)> {
    let src = src.trim();
    if let Some(rest) = src.strip_prefix("data:") {
        let (_, data) = rest.split_once(";base64,")?;
        let compact: String = data.chars().filter(|c| !c.is_whitespace()).collect();
        use base64::Engine as _;
        let bytes = base64::engine::general_purpose::STANDARD.decode(compact).ok()?;
        let kind = sniff_image_kind(&bytes)?;
        return Some((bytes, kind.to_string()));
    }
    if src.starts_with("http://") || src.starts_with("https://") {
        return None;
    }
    let path = src.strip_prefix("file://").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(src));
    let path = if path.is_absolute() { path } else { base_dir.join(&path) };
    let bytes = std::fs::read(path).ok()?;
    let kind = sniff_image_kind(&bytes)?;
    Some((bytes, kind.to_string()))
}

pub fn image_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    image::load_from_memory(bytes).ok().map(|img| (img.width(), img.height()))
}

// ---------------------------------------------------------------------------
// Markdown + HTML
// ---------------------------------------------------------------------------

pub fn export_markdown(docs: &[SheetDoc]) -> String {
    docs.iter().map(|d| d.content.clone()).collect::<Vec<_>>().join("\n\n---\n\n")
}

fn spans_html(spans: &[Span]) -> String {
    let mut out = String::new();
    for s in spans {
        let mut t = escape_html(&s.text);
        if s.code {
            t = format!("<code>{t}</code>");
        }
        if s.bold {
            t = format!("<strong>{t}</strong>");
        }
        if s.italic {
            t = format!("<em>{t}</em>");
        }
        if let Some(url) = &s.href {
            t = format!("<a href=\"{}\">{t}</a>", escape_html(url));
        }
        out.push_str(&t);
    }
    out
}

pub fn blocks_html_fragment(blocks: &[Block], images: &mut ImageStore, base_dir: &Path) -> String {
    let mut out = String::new();
    for b in blocks {
        match b {
            Block::Heading(n, t) => {
                let lvl = (*n).clamp(1, 6);
                out.push_str(&format!("<h{lvl}>{}</h{lvl}>\n", spans_html(&parse_spans(t))));
            }
            Block::Bullet(t) => {
                out.push_str(&format!("<ul>\n<li>{}</li>\n</ul>\n", spans_html(&parse_spans(t))));
            }
            Block::Numbered(n, t) => {
                out.push_str(&format!("<ol start=\"{n}\">\n<li>{}</li>\n</ol>\n", spans_html(&parse_spans(t))));
            }
            Block::Task(done, t) => {
                let box_ = if *done { "☑" } else { "☐" };
                out.push_str(&format!("<p>{box_} {}</p>\n", spans_html(&parse_spans(t))));
            }
            Block::Quote(t) => {
                out.push_str(&format!("<blockquote>\n<p>{}</p>\n</blockquote>\n", spans_html(&parse_spans(t))));
            }
            Block::Code(c) => {
                out.push_str(&format!("<pre><code>{}</code></pre>\n", escape_html(c.trim_end_matches('\n'))));
            }
            Block::Hr => out.push_str("<hr>\n"),
            Block::Para(t) => {
                out.push_str(&format!("<p>{}</p>\n", spans_html(&parse_spans(t))));
            }
            Block::Image { alt, src } => {
                if let Some((bytes, kind)) = resolve_image_bytes(base_dir, src) {
                    let idx = images.add(src, bytes, &kind);
                    let uri = &images.items[idx].data_uri;
                    out.push_str(&format!("<p><img src=\"{uri}\" alt=\"{}\"></p>\n", escape_html(alt)));
                } else {
                    out.push_str(&format!("<p>[image: {}]</p>\n", escape_html(alt)));
                }
            }
        }
    }
    out
}

pub fn export_html(docs: &[SheetDoc], css: &str, stem: &str) -> String {
    let theme = ExportTheme::from_css(css);
    let mut images = ImageStore::default();
    let mut body = String::new();
    for (k, doc) in docs.iter().enumerate() {
        if k > 0 {
            body.push_str("<hr class=\"sheet-gap\">\n");
        }
        let title = inferred_title(&doc.content);
        body.push_str(&format!("<h1 class=\"sheet-title\">{}</h1>\n", escape_html(&title)));
        body.push_str(&blocks_html_fragment(&export_blocks(&doc.content), &mut images, &doc.base_dir));
    }
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>\nbody {{ font-family: -apple-system, Georgia, serif; max-width: 42em; margin: 2em auto; padding: 0 1em; color: #222; line-height: 1.6; }}\npre {{ background: #f4f4f4; padding: 1em; overflow-x: auto; }}\nblockquote {{ border-left: 3px solid #ccc; margin-left: 0; padding-left: 1em; color: #555; }}\n.sheet-gap {{ margin: 3em 0; }}\nimg {{ max-width: 100%; }}\n{}\n</style>\n</head>\n<body>\n{body}</body>\n</html>\n",
        escape_html(stem),
        theme.raw_css,
    )
}

// ---------------------------------------------------------------------------
// RTF
// ---------------------------------------------------------------------------

fn rtf_align_word(align: ExportAlign) -> &'static str {
    match align {
        ExportAlign::Left => "",
        ExportAlign::Center => "\\qc ",
        ExportAlign::Right => "\\qr ",
        ExportAlign::Justify => "\\qj ",
    }
}

fn spans_rtf(spans: &[Span], link_cf: Option<usize>) -> String {
    fn esc_chunk(s: &str) -> String {
        let mut out = String::new();
        for c in s.chars() {
            match c {
                '{' => out.push_str("\\{"),
                '}' => out.push_str("\\}"),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\line "),
                c if (c as u32) > 127 => out.push_str(&format!("\\u{}?", c as u32)),
                c => out.push(c),
            }
        }
        out
    }
    let mut out = String::new();
    for s in spans {
        let mut t = esc_chunk(&s.text);
        if let Some(url) = &s.href {
            t = format!("{} ({})", t, esc_chunk(url));
            if let Some(cf) = link_cf {
                t = format!("{{\\cf{cf} {t}}}");
            }
        }
        if s.code {
            t = format!("{{\\f1 {t}}}");
        }
        if s.bold {
            t = format!("{{\\b {t}}}");
        }
        if s.italic {
            t = format!("{{\\i {t}}}");
        }
        out.push_str(&t);
    }
    out
}

pub fn export_rtf(docs: &[SheetDoc], css: &str, attached: &[String]) -> (String, Vec<String>) {
    let theme = ExportTheme::from_css(css);
    let (resolver, mut warnings) = FontResolver::new(attached);
    // Resolve display names once per used slot; warnings stay deduplicated.
    let used = used_theme_slots(docs);
    let slots = [
        &theme.body,
        &theme.title,
        &theme.h1,
        &theme.h2,
        &theme.h3,
        &theme.quote,
        &theme.code,
    ];
    let mut warned: HashSet<String> = HashSet::new();
    let mut resolved: Vec<String> = Vec::with_capacity(7);
    for (i, st) in slots.iter().enumerate() {
        let mono = st.pdf_family() == PdfFamily::Mono;
        if used[i] {
            let (name, w) = resolver.display(&st.family_stack, st.bold, mono);
            if let Some(w) = w {
                if warned.insert(w.clone()) {
                    warnings.push(w);
                }
            }
            resolved.push(name);
        } else {
            // Unused slot: still needs a table entry, but stays silent.
            let (name, _) = resolver.display(&st.family_stack, st.bold, mono);
            resolved.push(name);
        }
    }
    // resolved order: body, title, h1, h2, h3, quote, code
    let mut palette: Vec<(u8, u8, u8)> = Vec::new();
    for c in [theme.body.color, theme.title.color, theme.h1.color, theme.h2.color, theme.h3.color, theme.quote.color, theme.code.color, theme.link_color].into_iter().flatten() {
        if !palette.contains(&c) {
            palette.push(c);
        }
    }
    let cf = |c: Option<(u8, u8, u8)>| -> Option<usize> {
        c.and_then(|cc| palette.iter().position(|p| *p == cc).map(|i| i + 1))
    };
    let link_cf = cf(theme.link_color);
    let fs = |size_pt: f32| (size_pt * 2.0).round() as u32;
    let mut fonts: Vec<String> = vec!["Helvetica".to_string(), "Courier New".to_string()];
    for name in &resolved {
        if !fonts.contains(name) {
            fonts.push(name.clone());
        }
    }
    let rf = |fam: Option<&str>| -> Option<usize> {
        fam.and_then(|f| fonts.iter().position(|x| x == f))
    };
    // Index into `resolved` by theme slot.
    let (rb, rt, rh1, rh2, rh3, rq, rc) = (
        resolved[0].clone(),
        resolved[1].clone(),
        resolved[2].clone(),
        resolved[3].clone(),
        resolved[4].clone(),
        resolved[5].clone(),
        resolved[6].clone(),
    );
    let block = |style: &ElStyle, body: String, force_bold: bool, font: Option<usize>, page_break: bool, tail: &str| -> String {
        let mut body = body;
        if style.italic {
            body = format!("{{\\i {body}}}");
        }
        if let Some(n) = cf(style.color) {
            body = format!("{{\\cf{n} {body}}}");
        }
        let after = style.space_after_pt.map(|pt| format!("\\sa{} ", (pt * 20.0) as u32)).unwrap_or_default();
        let mut indent = String::new();
        if let Some(l) = style.margin_left {
            indent.push_str(&format!("\\li{} ", (l * 20.0) as i32));
        }
        if let Some(r) = style.margin_right {
            indent.push_str(&format!("\\ri{} ", (r * 20.0) as i32));
        }
        if let Some(f) = style.text_indent {
            indent.push_str(&format!("\\fi{} ", (f * 20.0) as i32));
        }
        format!(
            "{{{pb}{ind}{a}{after}\\fs{s}{}{} {body}}}{tail}",
            match font {
                Some(f) => format!("\\f{f} "),
                None => String::new(),
            },
            if force_bold || style.bold { "\\b " } else { "" },
            pb = if page_break { "\\pagebb " } else { "" },
            ind = indent,
            a = rtf_align_word(style.align),
            s = fs(style.size_pt),
        )
    };
    let mut out = String::from("{\\rtf1\\ansi\\deff0{\\fonttbl{\\f0 Helvetica;}{\\f1 Courier New;}");
    for (i, name) in fonts.iter().enumerate().skip(2) {
        out.push_str(&format!("{{\\f{i} {name};}}"));
    }
    out.push_str("}");
    if !palette.is_empty() {
        out.push_str("{\\colortbl;");
        for (r, g, b) in &palette {
            out.push_str(&format!("\\red{r}\\green{g}\\blue{b};"));
        }
        out.push('}');
    }
    if let Some(page) = theme.page.as_ref() {
        if let Some((w, h)) = page.size {
            out.push_str(&format!("\\paperw{} \\paperh{}", (w * 20.0) as u32, (h * 20.0) as u32));
        }
        let sides = [(page.margin[3], "\\margl"), (page.margin[1], "\\margr"), (page.margin[0], "\\margt"), (page.margin[2], "\\margb")];
        for (v, tag) in sides {
            if let Some(pt) = v {
                out.push_str(&format!("{tag}{} ", (pt * 20.0) as u32));
            }
        }
    }
    let mut first_para = true;
    let mut pending_break = false;
    let mut push_para = |out: &mut String, style: &ElStyle, text: String, force_bold: bool, font: Option<usize>| {
        let pb = (style.break_before || pending_break) && !first_para;
        pending_break = style.break_after;
        first_para = false;
        out.push_str(&block(style, text, force_bold, font, pb, "\\par\n"));
    };
    for (k, doc) in docs.iter().enumerate() {
        if k > 0 {
            push_para(&mut out, &theme.body, String::new(), false, rf(Some(&rb)));
            push_para(&mut out, &theme.body, "---".to_string(), false, rf(Some(&rb)));
            push_para(&mut out, &theme.body, String::new(), false, rf(Some(&rb)));
        }
        push_para(&mut out, &theme.title, spans_rtf(&parse_spans(&inferred_title(&doc.content)), link_cf), true, rf(Some(&rt)));
        for b in export_blocks(&doc.content) {
            let (text, style, bold, font) = match &b {
                Block::Heading(n, t) => {
                    let st = if *n <= 1 { &theme.h1 } else if *n == 2 { &theme.h2 } else { &theme.h3 };
                    let rn = if *n <= 1 { &rh1 } else if *n == 2 { &rh2 } else { &rh3 };
                    (spans_rtf(&parse_spans(t), link_cf), st, true, rf(Some(rn)))
                }
                Block::Bullet(t) => (format!("\\u8226?  {}", spans_rtf(&parse_spans(t), link_cf)), &theme.body, false, rf(Some(&rb))),
                Block::Numbered(n, t) => (format!("{n}.  {}", spans_rtf(&parse_spans(t), link_cf)), &theme.body, false, rf(Some(&rb))),
                Block::Task(done, t) => {
                    let mark = if *done { "\\u2611?" } else { "\\u2610?" };
                    (format!("{mark}  {}", spans_rtf(&parse_spans(t), link_cf)), &theme.body, false, rf(Some(&rb)))
                }
                Block::Quote(t) => (spans_rtf(&parse_spans(t), link_cf), &theme.quote, false, rf(Some(&rq))),
                Block::Code(c) => {
                    for line in c.lines() {
                        let st = &theme.code;
                        push_para(&mut out, st, spans_rtf(&[Span { text: line.to_string(), bold: false, italic: false, code: true, href: None }], link_cf), false, rf(Some(&rc)));
                    }
                    continue;
                }
                Block::Hr => ("---".to_string(), &theme.body, false, rf(Some(&rb))),
                Block::Para(t) => (spans_rtf(&parse_spans(t), link_cf), &theme.body, false, rf(Some(&rb))),
                Block::Image { alt, .. } => (format!("[image: {}]", alt), &theme.body, false, rf(Some(&rb))),
            };
            let st = style.clone();
            push_para(&mut out, &st, text, bold, font);
        }
    }
    out.push('}');
    (out, warnings)
}

// ---------------------------------------------------------------------------
// ZIP (stored, no compression — valid for docx/epub)
// ---------------------------------------------------------------------------

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 == 1 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub fn build_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    fn w16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    fn w32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let crc = crc32(data);
        let head = out.len() as u32;
        let len = data.len() as u32;
        w32(&mut out, 0x0403_4B50);
        w16(&mut out, 20);
        w16(&mut out, 0x0800);
        w16(&mut out, 0);
        w16(&mut out, 0);
        w16(&mut out, 0);
        w32(&mut out, crc);
        w32(&mut out, len);
        w32(&mut out, len);
        w16(&mut out, name.len() as u16);
        w16(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        w32(&mut central, 0x0201_4B50);
        w16(&mut central, 20);
        w16(&mut central, 20);
        w16(&mut central, 0x0800);
        w16(&mut central, 0);
        w16(&mut central, 0);
        w16(&mut central, 0);
        w32(&mut central, crc);
        w32(&mut central, len);
        w32(&mut central, len);
        w16(&mut central, name.len() as u16);
        w16(&mut central, 0);
        w16(&mut central, 0);
        w16(&mut central, 0);
        w16(&mut central, 0);
        w32(&mut central, 0);
        w32(&mut central, head);
        central.extend_from_slice(name.as_bytes());
    }
    let cstart = out.len() as u32;
    out.extend_from_slice(&central);
    let cend = out.len() as u32;
    w32(&mut out, 0x0605_4B50);
    w16(&mut out, 0);
    w16(&mut out, 0);
    w16(&mut out, files.len() as u16);
    w16(&mut out, files.len() as u16);
    w32(&mut out, cend - cstart);
    w32(&mut out, cstart);
    w16(&mut out, 0);
    out
}

// ---------------------------------------------------------------------------
// DOCX
// ---------------------------------------------------------------------------

fn docx_align(align: ExportAlign) -> Option<&'static str> {
    match align {
        ExportAlign::Left => None,
        ExportAlign::Center => Some("center"),
        ExportAlign::Right => Some("right"),
        ExportAlign::Justify => Some("both"),
    }
}

fn docx_color(c: Option<(u8, u8, u8)>) -> String {
    c.map(|(r, g, b)| format!("{:02X}{:02X}{:02X}", r, g, b)).unwrap_or_default()
}

fn docx_runs(spans: &[Span], size_pt: f32, base_bold: bool, base_italic: bool, color: &str, family: Option<&str>) -> String {
    let mut out = String::new();
    let sz = (size_pt * 2.0).round() as u32;
    for s in spans {
        let b = base_bold || s.bold;
        let i = base_italic || s.italic;
        let mut rpr = format!("<w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/>");
        if b {
            rpr.push_str("<w:b/><w:bCs/>");
        }
        if i {
            rpr.push_str("<w:i/><w:iCs/>");
        }
        if !color.is_empty() {
            rpr.push_str(&format!("<w:color w:val=\"{color}\"/>"));
        }
        if let Some(f) = family {
            let f = escape_xml(f);
            rpr.push_str(&format!("<w:rFonts w:ascii=\"{f}\" w:hAnsi=\"{f}\" w:cs=\"{f}\"/>"));
        }
        if s.code {
            rpr.push_str("<w:rFonts w:ascii=\"Courier New\" w:hAnsi=\"Courier New\"/>");
        }
        let mut text = escape_xml(&s.text);
        if let Some(url) = &s.href {
            text = format!("{text} ({})", escape_xml(url));
        }
        out.push_str(&format!("<w:r><w:rPr>{rpr}</w:rPr><w:t xml:space=\"preserve\">{text}</w:t></w:r>"));
    }
    out
}

pub fn export_docx_bytes(docs: &[SheetDoc], css: &str, attached: &[String]) -> (Vec<u8>, Vec<String>) {
    let theme = ExportTheme::from_css(css);
    let (resolver, mut warnings) = FontResolver::new(attached);
    // Missing-font warnings only for slots the export actually uses.
    {
        let used = used_theme_slots(docs);
        let slots = [
            &theme.body,
            &theme.title,
            &theme.h1,
            &theme.h2,
            &theme.h3,
            &theme.quote,
            &theme.code,
        ];
        let mut warned: HashSet<String> = HashSet::new();
        for (i, st) in slots.iter().enumerate() {
            if !used[i] {
                continue;
            }
            let mono = st.pdf_family() == PdfFamily::Mono;
            if let (_, Some(w)) = resolver.display(&st.family_stack, st.bold, mono) {
                if warned.insert(w.clone()) {
                    warnings.push(w);
                }
            }
        }
    }
    // Resolved display name for a style (warnings already collected above).
    let dname = |st: &ElStyle| -> String {
        let mono = st.pdf_family() == PdfFamily::Mono;
        resolver.display(&st.family_stack, st.bold, mono).0
    };
    let mut images = ImageStore::default();
    // Resolve images first (deduped).
    for doc in docs {
        for b in export_blocks(&doc.content) {
            if let Block::Image { src, .. } = b {
                if let Some((bytes, kind)) = resolve_image_bytes(&doc.base_dir, &src) {
                    images.add(&src, bytes, &kind);
                }
            }
        }
    }
    let (page_w, page_h) = theme.page_dims();
    let pg_w = (page_w * 20.0) as u32;
    let pg_h = (page_h * 20.0) as u32;
    let margins = [
        theme.page_margin(0).unwrap_or(1440.0),
        theme.page_margin(1).unwrap_or(1440.0),
        theme.page_margin(2).unwrap_or(1440.0),
        theme.page_margin(3).unwrap_or(1440.0),
    ];
    let mut paras: Vec<String> = Vec::new();
    let mut media: Vec<(String, Vec<u8>, &'static str)> = Vec::new();
    for (name, item) in images.items.iter().enumerate() {
        let fname = format!("image{name}.{}", if item.mime == "image/jpeg" { "jpg" } else { item.mime.split('/').nth(1).unwrap_or("png") });
        media.push((fname, item.bytes.clone(), item.mime));
    }
    let media_name = |src: &str, images: &ImageStore| -> Option<(usize, String)> {
        images.by_src.get(src).map(|&i| {
            let mime = images.items[i].mime;
            let ext = if mime == "image/jpeg" { "jpg" } else { mime.split('/').nth(1).unwrap_or("png") };
            (i, format!("image{i}.{ext}"))
        })
    };

    let mut first = true;
    let mut pending_break = false;
    macro_rules! para {
        ($style:expr, $inner:expr, $force_bold:expr) => {{
            let style: &ElStyle = $style;
            let inner: String = $inner;
            let force_bold: bool = $force_bold;
            let pb = (style.break_before || pending_break) && !first;
            pending_break = style.break_after;
            first = false;
            let mut ppr = String::new();
            if pb {
                ppr.push_str("<w:pageBreakBefore/>");
            }
            if let Some(a) = docx_align(style.align) {
                ppr.push_str(&format!("<w:jc w:val=\"{a}\"/>"));
            }
            let after = style.space_after_pt.unwrap_or(style.size_pt * 0.45);
            ppr.push_str(&format!("<w:spacing w:after=\"{}\" w:line=\"{} \" w:lineRule=\"auto\"/>", (after * 20.0) as u32, (style.line_height.unwrap_or(1.35) * 240.0) as u32));
            if let Some(l) = style.margin_left {
                ppr.push_str(&format!("<w:ind w:left=\"{}\"", (l * 20.0) as u32));
                if let Some(r) = style.margin_right {
                    ppr.push_str(&format!(" w:right=\"{}\"", (r * 20.0) as u32));
                }
                if let Some(f) = style.text_indent {
                    ppr.push_str(&format!(" w:firstLine=\"{}\"", (f * 20.0) as u32));
                }
                ppr.push_str("/>");
            } else if style.text_indent.is_some() || style.margin_right.is_some() {
                ppr.push_str("<w:ind");
                if let Some(f) = style.text_indent {
                    ppr.push_str(&format!(" w:firstLine=\"{}\"", (f * 20.0) as u32));
                }
                if let Some(r) = style.margin_right {
                    ppr.push_str(&format!(" w:right=\"{}\"", (r * 20.0) as u32));
                }
                ppr.push_str("/>");
            }
            let color = docx_color(style.color);
            let runs = if inner.starts_with("<w:r") || inner.starts_with("<w:hyperlink") { inner } else { docx_runs(&parse_spans(&inner), style.size_pt, force_bold || style.bold, style.italic, &color, Some(dname(style).as_str())) };
            paras.push(format!("<w:p><w:pPr>{ppr}<w:rPr><w:sz w:val=\"{}\"/><w:szCs w:val=\"{}\"/></w:rPr></w:pPr>{runs}</w:p>", (style.size_pt*2.0).round() as u32, (style.size_pt*2.0).round() as u32));
        }};
    }

    let mut doc_rel_id = 1u32;
    let mut rels = String::new();
    for (k, doc) in docs.iter().enumerate() {
        if k > 0 {
            para!(&theme.body, String::new(), false);
            para!(&theme.body, "---".to_string(), false);
            para!(&theme.body, String::new(), false);
        }
        para!(&theme.title, escape_xml(&inferred_title(&doc.content)), true);
        for b in export_blocks(&doc.content) {
            match b {
                Block::Heading(n, t) => {
                    let st = if n <= 1 { &theme.h1 } else if n == 2 { &theme.h2 } else { &theme.h3 };
                    let color = docx_color(st.color);
                    let runs = docx_runs(&parse_spans(&t), st.size_pt, true, st.italic, &color, Some(dname(st).as_str()));
                    para!(st, runs, true);
                }
                Block::Bullet(t) => {
                    let color = docx_color(theme.body.color);
                    let runs = docx_runs(&parse_spans(&format!("•  {t}")), theme.body.size_pt, false, theme.body.italic, &color, Some(dname(&theme.body).as_str()));
                    para!(&theme.body, runs, false);
                }
                Block::Numbered(n, t) => {
                    let color = docx_color(theme.body.color);
                    let runs = docx_runs(&parse_spans(&format!("{n}.  {t}")), theme.body.size_pt, false, theme.body.italic, &color, Some(dname(&theme.body).as_str()));
                    para!(&theme.body, runs, false);
                }
                Block::Task(done, t) => {
                    let mark = if done { "☑" } else { "☐" };
                    let color = docx_color(theme.body.color);
                    let runs = docx_runs(&parse_spans(&format!("{mark}  {t}")), theme.body.size_pt, false, theme.body.italic, &color, Some(dname(&theme.body).as_str()));
                    para!(&theme.body, runs, false);
                }
                Block::Quote(t) => {
                    let color = docx_color(theme.quote.color);
                    let runs = docx_runs(&parse_spans(&t), theme.quote.size_pt, theme.quote.bold, true, &color, Some(dname(&theme.quote).as_str()));
                    para!(&theme.quote, runs, false);
                }
                Block::Code(c) => {
                    for line in c.lines() {
                        let runs = docx_runs(&[Span { text: line.to_string(), bold: false, italic: false, code: true, href: None }], theme.code.size_pt, false, false, &docx_color(theme.code.color), Some(dname(&theme.code).as_str()));
                        para!(&theme.code, runs, false);
                    }
                }
                Block::Hr => para!(&theme.body, "---".to_string(), false),
                Block::Para(t) => {
                    let color = docx_color(theme.body.color);
                    let runs = docx_runs(&parse_spans(&t), theme.body.size_pt, false, theme.body.italic, &color, Some(dname(&theme.body).as_str()));
                    para!(&theme.body, runs, false);
                }
                Block::Image { alt, src } => {
                    if let Some((idx, fname)) = media_name(&src, &images) {
                        let kind = images.items[idx].mime;
                        let ext = if kind == "image/jpeg" { "jpg" } else { kind.split('/').nth(1).unwrap_or("png") };
                        let _ = ext;
                        let rid = format!("rId{doc_rel_id}");
                        doc_rel_id += 1;
                        rels.push_str(&format!("<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"media/{fname}\"/>"));
                        // Scale to content width (~6in max) preserving aspect.
                        let (w_px, h_px) = image_dimensions(&images.items[idx].bytes).unwrap_or((600, 400));
                        let max_w_emu = 5_800_000i64;
                        let emu_per_px = 9525i64;
                        let mut w = w_px as i64 * emu_per_px;
                        let mut h = h_px as i64 * emu_per_px;
                        if w > max_w_emu {
                            h = h * max_w_emu / w;
                            w = max_w_emu;
                        }
                        if h > 7_000_000 {
                            w = w * 7_000_000 / h;
                            h = 7_000_000;
                        }
                        let alt_esc = escape_xml(if alt.is_empty() { "image" } else { &alt });
                        let drawing = format!("<w:r><w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{w}\" cy=\"{h}\"/><wp:docPr id=\"{idx}\" name=\"{alt_esc}\"/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"/></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>");
                        paras.push(format!("<w:p><w:pPr></w:pPr>{drawing}</w:p>"));
                    } else {
                        para!(&theme.body, format!("[image: {alt}]"), false);
                    }
                }
            }
        }
    }

    let sect = format!("<w:sectPr><w:pgSz w:w=\"{pg_w}\" w:h=\"{pg_h}\"/><w:pgMar w:top=\"{}\" w:right=\"{}\" w:bottom=\"{}\" w:left=\"{}\" w:header=\"720\" w:footer=\"720\"/></w:sectPr>", margins[0] as u32, margins[1] as u32, margins[2] as u32, margins[3] as u32);
    paras.push(format!("<w:p><w:pPr>{sect}</w:pPr></w:p>"));
    let document = format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>{}<w:p><w:r><w:t></w:t></w:r></w:p></w:body></w:document>", paras.join(""));
    let content_types = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"gif\" ContentType=\"image/gif\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>");
    let rels_root = "<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
    let doc_rels = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}</Relationships>");
    let mut files: Vec<(&str, &[u8])> = vec![
        ("[Content_Types].xml", content_types.as_bytes()),
        ("_rels/.rels", rels_root.as_bytes()),
        ("word/document.xml", document.as_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.as_bytes()),
    ];
    let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
    for (fname, bytes, _) in &media {
        owned.push((format!("word/media/{fname}"), bytes.clone()));
    }
    for (n, b) in &owned {
        files.push((n.as_str(), b.as_slice()));
    }
    (build_zip(&files), warnings)
}

// ---------------------------------------------------------------------------
// EPUB
// ---------------------------------------------------------------------------

pub fn export_epub_bytes(docs: &[SheetDoc], css: &str, stem: &str) -> Vec<u8> {
    let theme = ExportTheme::from_css(css);
    let mut images = ImageStore::default();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut manifest_items: Vec<String> = Vec::new();
    let mut spine_items: Vec<String> = Vec::new();
    for (i, doc) in docs.iter().enumerate() {
        let title = inferred_title(&doc.content);
        let body = blocks_html_fragment(&export_blocks(&doc.content), &mut images, &doc.base_dir);
        let file = format!("text{sec}.xhtml", sec = i);
        let page = format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\">\n<head><title>{}</title><link rel=\"stylesheet\" type=\"text/css\" href=\"style.css\"/></head>\n<body>\n<h1>{}</h1>\n{body}</body>\n</html>\n", escape_html(&title), escape_html(&title));
        sections.push((file, page));
    }
    for (i, _) in sections.iter().enumerate() {
        manifest_items.push(format!("<item id=\"sec{i}\" href=\"text{sec}.xhtml\" media-type=\"application/xhtml+xml\"/>", sec = i));
        spine_items.push(format!("<itemref idref=\"sec{i}\"/>"));
    }
    for item in &images.items {
        let ext = item.mime.split('/').nth(1).unwrap_or("png");
        manifest_items.push(format!("<item id=\"{}\" href=\"images/{}\" media-type=\"{}\"/>", item.name, item.name.replace("img", "img"), item.mime));
        let _ = ext;
    }
    let css_text = format!("body {{ font-family: serif; line-height: 1.6; }}\nimg {{ max-width: 100%; }}\n{}", theme.raw_css);
    let opf = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" unique-identifier=\"bookid\" version=\"2.0\">\n<metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:language>en</dc:language><dc:identifier id=\"bookid\">alcott-{}</dc:identifier></metadata>\n<manifest>\n<item id=\"ncx\" href=\"toc.ncx\" media-type=\"application/x-dtbncx+xml\"/>\n<item id=\"css\" href=\"style.css\" media-type=\"text/css\"/>\n{}\n</manifest>\n<spine toc=\"ncx\">\n{}\n</spine>\n</package>\n", escape_xml(stem), escape_xml(stem), manifest_items.join("\n"), spine_items.join("\n"));
    let mut nav_points = String::new();
    for (i, doc) in docs.iter().enumerate() {
        nav_points.push_str(&format!("<navPoint id=\"nav{i}\" playOrder=\"{}\"><navLabel><text>{}</text></navLabel><content src=\"text{sec}.xhtml\"/></navPoint>\n", i + 1, escape_xml(&inferred_title(&doc.content)), sec = i));
    }
    let ncx = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\">\n<head><meta name=\"dtb:uid\" content=\"alcott-{}\"/></head>\n<docTitle><text>{}</text></docTitle>\n<navMap>\n{}</navMap>\n</ncx>\n", escape_xml(stem), escape_xml(stem), nav_points);
    let container = "<?xml version=\"1.0\"?><container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
    let mut files: Vec<(&str, &[u8])> = vec![
        ("mimetype", b"application/epub+zip".as_slice()),
        ("META-INF/container.xml", container.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/toc.ncx", ncx.as_bytes()),
        ("OEBPS/style.css", css_text.as_bytes()),
    ];
    let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
    for (file, page) in &sections {
        owned.push((format!("OEBPS/{file}"), page.as_bytes().to_vec()));
    }
    for item in &images.items {
        owned.push((format!("OEBPS/images/{}", item.name), item.bytes.clone()));
    }
    for (n, b) in &owned {
        files.push((n.as_str(), b.as_slice()));
    }
    build_zip(&files)
}

// ---------------------------------------------------------------------------
// Fonts (attached files + system), mirrors Ariadne
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct EmbeddedFont {
    pub family: String,
    pub weight: fontdb::Weight,
    pub bytes: Vec<u8>,
    pub index: u32,
}

pub fn font_key(name: &str) -> String {
    name.trim().to_lowercase()
}

fn face_bytes(db: &fontdb::Database, id: fontdb::ID) -> Option<(Vec<u8>, u32)> {
    let (source, index) = db.face_source(id)?;
    let data: Vec<u8> = match source {
        fontdb::Source::Binary(data) => AsRef::<[u8]>::as_ref(data.as_ref()).to_vec(),
        fontdb::Source::File(path) => std::fs::read(path).ok()?,
        fontdb::Source::SharedFile(_, data) => AsRef::<[u8]>::as_ref(data.as_ref()).to_vec(),
    };
    Some((data, index))
}

pub fn probe_font_file(path: &str) -> Option<(String, Vec<EmbeddedFont>)> {
    let bytes = std::fs::read(path).ok()?;
    let mut probe = fontdb::Database::new();
    probe.load_font_data(bytes.clone());
    let mut faces: Vec<EmbeddedFont> = probe.faces().map(|info| EmbeddedFont {
        family: info.families.first().map(|(n, _)| n.clone()).unwrap_or_else(|| path.to_string()),
        weight: info.weight,
        bytes: bytes.clone(),
        index: info.index,
    }).collect();
    if faces.is_empty() {
        return None;
    }
    faces.sort_by_key(|f| f.index);
    let name = faces[0].family.clone();
    Some((name, faces))
}

/// Load attached font files into a family-keyed map. Unreadable files are
/// reported as warnings, never fatal.
pub fn attached_font_map(attached: &[String]) -> (HashMap<String, Vec<EmbeddedFont>>, Vec<String>) {
    let mut map: HashMap<String, Vec<EmbeddedFont>> = HashMap::new();
    let mut problems = Vec::new();
    for path in attached {
        match probe_font_file(path) {
            Some((_, faces)) => {
                for face in faces {
                    map.entry(font_key(&face.family)).or_default().push(face);
                }
            }
            None => problems.push(format!("Font file not found or unreadable: {path}")),
        }
    }
    (map, problems)
}

fn pick_attached<'a>(faces: &'a [EmbeddedFont], bold: bool) -> Option<&'a EmbeddedFont> {
    faces.iter().find(|f| (f.weight == fontdb::Weight::BOLD) == bold).or_else(|| faces.first())
}

fn query_system_font(db: &fontdb::Database, family: &str, bold: bool) -> Option<EmbeddedFont> {
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    })?;
    let (bytes, index) = face_bytes(db, id)?;
    let info = db.face(id)?;
    Some(EmbeddedFont {
        family: info.families.first().map(|(n, _)| n.clone()).unwrap_or_else(|| family.to_string()),
        weight: info.weight,
        bytes,
        index,
    })
}

fn query_generic_font(db: &fontdb::Database, mono: bool, bold: bool) -> Option<EmbeddedFont> {
    let id = db.query(&fontdb::Query {
        families: &[if mono { fontdb::Family::Monospace } else { fontdb::Family::SansSerif }],
        weight: if bold { fontdb::Weight::BOLD } else { fontdb::Weight::NORMAL },
        stretch: fontdb::Stretch::Normal,
        style: fontdb::Style::Normal,
    })?;
    let (bytes, index) = face_bytes(db, id)?;
    let info = db.face(id)?;
    Some(EmbeddedFont {
        family: info.families.first().map(|(n, _)| n.clone()).unwrap_or_default(),
        weight: info.weight,
        bytes,
        index,
    })
}

/// Generic CSS keywords carry no named family; they steer the generic
/// fallback and mono detection but are never looked up by name.
fn is_generic_family(name: &str) -> bool {
    matches!(
        name.trim().to_lowercase().as_str(),
        "serif"
            | "sans-serif"
            | "monospace"
            | "cursive"
            | "fantasy"
            | "system-ui"
            | "ui-sans-serif"
            | "ui-serif"
            | "ui-monospace"
            | "emoji"
    )
}

/// Generic keyword the author named, if any, so fallbacks keep their
/// intent (`serif` stays serif instead of becoming sans-serif).
fn generic_name(stack: &[String], mono: bool) -> String {
    for s in stack {
        match s.trim().to_lowercase().as_str() {
            "serif" | "sans-serif" | "monospace" | "cursive" | "fantasy" => {
                return s.trim().to_string()
            }
            _ => {}
        }
    }
    if mono { "monospace" } else { "sans-serif" }.to_string()
}

/// Install hint for a missing font, covering Debian/Ubuntu (apt),
/// Fedora (dnf), Arch (pacman) and NixOS (nixpkgs attribute).
pub fn font_install_hint(requested: &str) -> &'static str {
    match requested.trim().to_lowercase().as_str() {
        s if s.contains("helvetica") || s == "arial" => "Debian/Ubuntu: apt install fonts-liberation2; Fedora: dnf install liberation-sans-fonts; Arch: pacman -S ttf-liberation; NixOS: fonts.packages += pkgs.liberation_ttf",
        s if s.contains("times") => "Debian/Ubuntu: apt install fonts-liberation2; Fedora: dnf install liberation-serif-fonts; Arch: pacman -S ttf-liberation; NixOS: fonts.packages += pkgs.liberation_ttf",
        s if s.contains("courier") => "Debian/Ubuntu: apt install fonts-liberation2; Fedora: dnf install liberation-mono-fonts; Arch: pacman -S ttf-liberation; NixOS: fonts.packages += pkgs.liberation_ttf",
        s if s.contains("georgia") || s.contains("garamond") || s.contains("palatino") || s.contains("cambria") => "Debian/Ubuntu: apt install fonts-noto-core; Fedora: dnf install google-noto-serif-fonts; Arch: pacman -S noto-fonts; NixOS: fonts.packages += pkgs.noto-fonts",
        s if s.contains("menlo") || s.contains("sf mono") || s.contains("monaco") || s.contains("consola") => "Debian/Ubuntu: apt install fonts-dejavu-core; Fedora: dnf install dejavu-sans-mono-fonts; Arch: pacman -S ttf-dejavu; NixOS: fonts.packages += pkgs.dejavu_fonts",
        _ => "Debian/Ubuntu: apt install fonts-dejavu-core fonts-liberation2; Fedora: dnf install dejavu-sans-fonts liberation-sans-fonts; Arch: pacman -S ttf-dejavu ttf-liberation; NixOS: fonts.packages += pkgs.dejavu_fonts",
    }
}

/// The author's real request: first non-generic name in a stack.
/// Empty stacks (the Built-in look) carry no opinion and stay silent.
fn requested_name(stack: &[String]) -> Option<&str> {
    stack.iter().map(|s| s.as_str()).find(|s| !is_generic_family(s))
}

/// Warning when the embedded/named font isn't the author's first choice.
/// Reported on every export so missing system fonts stay visible.
fn substitution_warning(stack: &[String], winner: &str) -> Option<String> {
    let requested = requested_name(stack)?;
    if font_key(requested) == font_key(winner) {
        return None;
    }
    Some(format!(
        "font \"{requested}\" not installed, using \"{winner}\" instead ({})",
        font_install_hint(requested)
    ))
}

fn load_system_db() -> fontdb::Database {
    let mut db = fontdb::Database::new();
    if std::env::var("ALCOTT_NO_SYSTEM_FONTS").is_err() {
        db.load_system_fonts();
    }
    db
}

/// Attached files plus installed system fonts behind one stack-walking
/// lookup, so the first *installed* family wins on every format.
pub struct FontResolver {
    attached: HashMap<String, Vec<EmbeddedFont>>,
    db: fontdb::Database,
}

impl FontResolver {
    pub fn new(attached_files: &[String]) -> (Self, Vec<String>) {
        let (attached, warnings) = attached_font_map(attached_files);
        let db = load_system_db();
        (Self { attached, db }, warnings)
    }

    /// Walk the stack in author order: attached files, then system fonts.
    /// Returns the embedded font plus the stack entry it satisfied.
    pub fn embed(&self, stack: &[String], bold: bool) -> Option<(EmbeddedFont, String)> {
        for cand in stack {
            if is_generic_family(cand) {
                continue;
            }
            if let Some(faces) = self.attached.get(&font_key(cand)) {
                if let Some(f) = pick_attached(faces, bold) {
                    return Some((f.clone(), cand.clone()));
                }
            }
            if let Some(f) = query_system_font(&self.db, cand, bold) {
                return Some((f, cand.clone()));
            }
        }
        None
    }

    /// Name to write into Word/RTF plus an optional missing-font warning.
    /// Prefers the first installed stack entry; otherwise keeps the author's
    /// first choice (readers that have it still render correctly).
    pub fn display(&self, stack: &[String], bold: bool, mono: bool) -> (String, Option<String>) {
        if let Some((_, winner)) = self.embed(stack, bold) {
            let warn = substitution_warning(stack, &winner);
            return (winner, warn);
        }
        if let Some(req) = requested_name(stack) {
            let generic = generic_name(stack, mono);
            let warn = format!(
                "font \"{req}\" not installed, using generic {generic} ({})",
                font_install_hint(req)
            );
            return (req.to_string(), Some(warn));
        }
        (generic_name(stack, mono), None)
    }

    pub fn generic(&self, mono: bool, bold: bool) -> Option<EmbeddedFont> {
        query_generic_font(&self.db, mono, bold)
    }
}

/// Which theme slots actually appear in the export, in
/// [body, title, h1, h2, h3, quote, code] order. Body and title always do;
/// the rest only with matching blocks. Missing-font warnings are limited
/// to used slots so clean sheets stay quiet.
fn used_theme_slots(docs: &[SheetDoc]) -> [bool; 7] {
    // order: body, title, h1, h2, h3, quote, code
    let mut used = [true, true, false, false, false, false, false];
    for doc in docs {
        for b in export_blocks(&doc.content) {
            match b {
                Block::Heading(1, _) => used[2] = true,
                Block::Heading(2, _) => used[3] = true,
                Block::Heading(_, _) => used[4] = true,
                Block::Quote(_) => used[5] = true,
                Block::Code(_) => used[6] = true,
                _ => {}
            }
        }
    }
    used
}

// ---------------------------------------------------------------------------
// PDF via krilla
// ---------------------------------------------------------------------------

type PdfItem = (String, f32, bool, Option<(u8, u8, u8)>, ExportAlign, f32, f32, Vec<String>, PdfFamily, f32, f32, f32, bool, bool);
type PdfLine = (String, f32, bool, Option<(u8, u8, u8)>, ExportAlign, f32, f32, Vec<String>, PdfFamily, f32, f32, f32);

enum PdfBlock {
    Text(PdfItem),
    Image(PdfImageBlock),
}

#[derive(Clone)]
struct PdfImageBlock {
    #[allow(dead_code)]
    alt: String,
    bytes: Vec<u8>,
    kind: &'static str,
    w_px: u32,
    h_px: u32,
}

enum PdfPlaced {
    Text(PdfLine),
    Image(PdfPlacedImage),
}

struct PdfPlacedImage {
    bytes: Vec<u8>,
    kind: &'static str,
    x: f32,
    top: f32,
    w: f32,
    h: f32,
}

struct PdfFontPlan {
    slots: Vec<Vec<usize>>,
    fonts: Vec<EmbeddedFont>,
    img_slots: Vec<Vec<usize>>,
    images: Vec<(Vec<u8>, &'static str)>,
}

fn layout_pdf_pages(items: Vec<PdfBlock>, dims: (f32, f32), margins: [f32; 4]) -> Vec<Vec<PdfPlaced>> {
    let (page_w, page_h) = dims;
    let [mt, mr, mb, ml] = margins;
    let mut pages: Vec<Vec<PdfPlaced>> = vec![Vec::new()];
    let mut used = 0.0;
    let mut y = page_h - mt;
    let mut pending_break = false;
    for item in items {
        match item {
            PdfBlock::Text((text, size, bold, color, align, leading_mult, gap, family, pdf_family, ind_l, ind_f, ind_r, brk_before, brk_after)) => {
                if (brk_before || pending_break) && !pages.last().unwrap().is_empty() {
                    pages.push(Vec::new());
                    used = 0.0;
                    y = page_h - mt;
                }
                pending_break = brk_after;
                let usable = (page_w - ml - mr - ind_l - ind_r).max(10.0);
                let first_width = (usable - ind_f).max(10.0);
                let words: Vec<&str> = text.split_whitespace().collect();
                let mut lines: Vec<(String, bool)> = Vec::new();
                if words.is_empty() {
                    lines.push((String::new(), true));
                } else {
                    let mut cur = String::new();
                    let mut is_first = true;
                    for w in words {
                        let width = if is_first { first_width } else { usable };
                        let max_chars = ((width / (size * 0.5).max(1.0)) as usize).max(10);
                        if cur.is_empty() {
                            cur = w.to_string();
                        } else if cur.len() + 1 + w.len() <= max_chars {
                            cur.push(' ');
                            cur.push_str(w);
                        } else {
                            lines.push((std::mem::take(&mut cur), is_first));
                            is_first = false;
                            cur = w.to_string();
                        }
                    }
                    if !cur.is_empty() {
                        lines.push((cur, is_first));
                    }
                }
                for (line, is_first) in lines {
                    let leading = size * leading_mult;
                    if used + leading > page_h - mt - mb && !pages.last().unwrap().is_empty() {
                        pages.push(Vec::new());
                        used = 0.0;
                        y = page_h - mt;
                    }
                    y -= leading;
                    pages.last_mut().unwrap().push(PdfPlaced::Text((line, size, bold, color, align, leading, y, family.clone(), pdf_family, ind_l, if is_first { ind_f } else { 0.0 }, ind_r)));
                    used += leading;
                }
                used += gap;
            }
            PdfBlock::Image(img) => {
                if pending_break && !pages.last().unwrap().is_empty() {
                    pages.push(Vec::new());
                    used = 0.0;
                    y = page_h - mt;
                }
                pending_break = false;
                let max_w = (page_w - ml - mr).max(10.0);
                let max_h = (page_h - mt - mb).max(10.0).min(500.0);
                // 96dpi assumption: px -> pt.
                let mut w = img.w_px as f32 * 72.0 / 96.0;
                let mut h = img.h_px as f32 * 72.0 / 96.0;
                if w > max_w {
                    h = h * max_w / w;
                    w = max_w;
                }
                if h > max_h {
                    w = w * max_h / h;
                    h = max_h;
                }
                let need = 6.0 + h;
                if used + need > page_h - mt - mb && !pages.last().unwrap().is_empty() {
                    pages.push(Vec::new());
                    used = 0.0;
                    y = page_h - mt;
                }
                y -= 6.0;
                let top = y;
                y -= h;
                pages.last_mut().unwrap().push(PdfPlaced::Image(PdfPlacedImage { bytes: img.bytes.clone(), kind: img.kind, x: ml, top, w, h }));
                used += need;
            }
        }
    }
    pages
}

fn stack_key(stack: &[String]) -> String {
    stack.iter().map(|s| font_key(s)).collect::<Vec<_>>().join("")
}

fn plan_pdf_fonts(
    pages: &[Vec<PdfPlaced>],
    resolver: &FontResolver,
) -> (Option<PdfFontPlan>, Vec<String>) {
    let mut warnings = Vec::new();
    // Distinct (stack, bold) needs, plus whether the fallback is mono.
    let mut keys: Vec<(String, Vec<String>, bool, bool)> = Vec::new();
    for page in pages {
        for line in page {
            match line {
                PdfPlaced::Text(t) => {
                    let key = (stack_key(&t.7), t.7.clone(), t.2, t.8 == PdfFamily::Mono);
                    if !keys.iter().any(|k| k.0 == key.0 && k.2 == key.2) {
                        keys.push(key);
                    }
                }
                PdfPlaced::Image(_) => {}
            }
        }
    }
    let mut resolved: HashMap<(String, bool), EmbeddedFont> = HashMap::new();
    let mut warned: HashSet<String> = HashSet::new();
    for (skey, stack, bold, mono) in &keys {
        if let Some((font, winner)) = resolver.embed(stack, *bold) {
            if let Some(w) = substitution_warning(stack, &winner) {
                if warned.insert(w.clone()) {
                    warnings.push(w);
                }
            }
            resolved.insert((skey.clone(), *bold), font);
            continue;
        }
        // Nothing in the stack is installed: generic fallback. Warn only
        // when the author actually named a font; empty stacks (Built-in)
        // carry no opinion and stay silent.
        match resolver.generic(*mono, *bold) {
            Some(font) => {
                if let Some(req) = requested_name(stack) {
                    if warned.insert(font_key(req)) {
                        warnings.push(format!(
                            "font \"{req}\" not installed, using generic fallback ({})",
                            font_install_hint(req)
                        ));
                    }
                }
                resolved.insert((skey.clone(), *bold), font);
            }
            None => {
                warnings.push(format!("No system font for '{skey}'; PDF may be incomplete"));
                // Synthesize a placeholder from any resolved font to keep layout.
                if let Some(any) = resolved.values().next().cloned() {
                    resolved.insert((skey.clone(), *bold), any);
                } else {
                    return (None, warnings);
                }
            }
        }
    }
    let mut fonts: Vec<EmbeddedFont> = Vec::new();
    let mut slots: Vec<Vec<usize>> = Vec::new();
    let mut images: Vec<(Vec<u8>, &'static str)> = Vec::new();
    let mut img_slots: Vec<Vec<usize>> = Vec::new();
    for page in pages {
        let mut row = Vec::new();
        let mut irow = Vec::new();
        for line in page {
            match line {
                PdfPlaced::Text(t) => {
                    let key = (stack_key(&t.7), t.2);
                    let font = resolved.get(&key).expect("planned above");
                    let slot = fonts.iter().position(|f: &EmbeddedFont| f.bytes == font.bytes && f.index == font.index).unwrap_or_else(|| {
                        fonts.push(font.clone());
                        fonts.len() - 1
                    });
                    row.push(slot);
                    irow.push(0);
                }
                PdfPlaced::Image(img) => {
                    row.push(0);
                    let slot = images.iter().position(|(b, _)| *b == img.bytes).unwrap_or_else(|| {
                        images.push((img.bytes.clone(), img.kind));
                        images.len() - 1
                    });
                    irow.push(slot);
                }
            }
        }
        slots.push(row);
        img_slots.push(irow);
    }
    (Some(PdfFontPlan { slots, fonts, img_slots, images }), warnings)
}

fn render_pdf_krilla(pages: &[Vec<PdfPlaced>], plan: &PdfFontPlan, dims: (f32, f32), margins: [f32; 4]) -> Option<Vec<u8>> {
    let (page_w, page_h) = dims;
    let [_, mr, _, ml] = margins;
    let mut kfonts: Vec<krilla::text::Font> = Vec::new();
    for font in &plan.fonts {
        kfonts.push(krilla::text::Font::new(font.bytes.clone().into(), font.index)?);
    }
    let mut kimages: Vec<krilla::image::Image> = Vec::new();
    for (bytes, kind) in &plan.images {
        let data: krilla::Data = bytes.clone().into();
        let img = match *kind {
            "png" => krilla::image::Image::from_png(data, true),
            "jpg" => krilla::image::Image::from_jpeg(data, true),
            "gif" => krilla::image::Image::from_gif(data, true),
            _ => return None,
        }
        .ok()?;
        kimages.push(img);
    }
    let mut doc = krilla::Document::new();
    for (pi, page) in pages.iter().enumerate() {
        let slots = &plan.slots[pi];
        let img_slots = &plan.img_slots[pi];
        let mut pg = doc.start_page_with(krilla::page::PageSettings::new(krilla::geom::Size::from_wh(page_w, page_h)?));
        let mut surface = pg.surface();
        for ((line, slot), islot) in page.iter().zip(slots.iter()).zip(img_slots.iter()) {
            match line {
                PdfPlaced::Text(t) => {
                    if t.0.is_empty() {
                        continue;
                    }
                    let font = kfonts.get(*slot)?;
                    let (r, g, b) = t.3.unwrap_or((0, 0, 0));
                    surface.set_fill(Some(krilla::paint::Fill {
                        paint: krilla::color::rgb::Color::new(r, g, b).into(),
                        opacity: krilla::num::NormalizedF32::ONE,
                        rule: Default::default(),
                    }));
                    let max_w = page_w - ml - mr;
                    let w_est = t.0.chars().count() as f32 * t.1 * 0.5;
                    let avail = max_w - t.9 - t.11 - t.10;
                    let x = ml + t.9 + t.10 + match t.4 {
                        ExportAlign::Left | ExportAlign::Justify => 0.0,
                        ExportAlign::Center => ((avail - w_est) / 2.0).max(0.0),
                        ExportAlign::Right => (avail - w_est).max(0.0),
                    };
                    surface.draw_text(
                        krilla::geom::Point::from_xy(x, page_h - t.6),
                        font.clone(),
                        t.1,
                        &t.0,
                        false,
                        krilla::text::TextDirection::Auto,
                    );
                }
                PdfPlaced::Image(img) => {
                    let krilla_img = kimages.get(*islot)?.clone();
                    surface.push_transform(&krilla::geom::Transform::from_translate(img.x, page_h - img.top));
                    surface.draw_image(krilla_img, krilla::geom::Size::from_wh(img.w, img.h)?);
                    surface.pop();
                }
            }
        }
        surface.finish();
        pg.finish();
    }
    doc.finish().ok()
}

pub fn export_pdf_bytes(docs: &[SheetDoc], css: &str, attached_fonts: &[String]) -> (Vec<u8>, Vec<String>) {
    let theme = ExportTheme::from_css(css);
    let mut warnings: Vec<String> = Vec::new();
    let mut items: Vec<PdfBlock> = Vec::new();
    let push = |items: &mut Vec<PdfBlock>, text: String, style: &ElStyle, bold: bool| {
        items.push(PdfBlock::Text((
            text, style.size_pt, bold, style.color, style.align,
            style.line_height.unwrap_or(1.35),
            style.space_after_pt.unwrap_or(style.size_pt * 0.45),
            style.family_stack.clone(), style.pdf_family(),
            style.margin_left.unwrap_or(0.0), style.text_indent.unwrap_or(0.0), style.margin_right.unwrap_or(0.0),
            style.break_before, style.break_after,
        )));
    };
    for (k, doc) in docs.iter().enumerate() {
        if k > 0 {
            push(&mut items, String::new(), &theme.body, false);
            push(&mut items, "---".to_string(), &theme.body, false);
            push(&mut items, String::new(), &theme.body, false);
        }
        push(&mut items, inferred_title(&doc.content), &theme.title, true);
        for b in export_blocks(&doc.content) {
            match b {
                Block::Heading(n, t) => {
                    let style = if n <= 1 { &theme.h1 } else if n == 2 { &theme.h2 } else { &theme.h3 };
                    push(&mut items, spans_plain(&parse_spans(&t)), style, true);
                }
                Block::Bullet(t) => push(&mut items, format!("•  {}", spans_plain(&parse_spans(&t))), &theme.body, false),
                Block::Numbered(n, t) => push(&mut items, format!("{n}.  {}", spans_plain(&parse_spans(&t))), &theme.body, false),
                Block::Task(done, t) => push(&mut items, format!("{}  {}", if done { "☑" } else { "☐" }, spans_plain(&parse_spans(&t))), &theme.body, false),
                Block::Quote(t) => push(&mut items, spans_plain(&parse_spans(&t)), &theme.quote, false),
                Block::Code(c) => {
                    for line in c.lines() {
                        push(&mut items, line.to_string(), &theme.code, false);
                    }
                }
                Block::Hr => push(&mut items, "---".to_string(), &theme.body, false),
                Block::Para(t) => push(&mut items, spans_plain(&parse_spans(&t)), &theme.body, false),
                Block::Image { alt, src } => {
                    match resolve_image_bytes(&doc.base_dir, &src) {
                        Some((bytes, kind)) => {
                            let kind_s: &'static str = match kind.as_str() {
                                "png" => "png",
                                "jpg" => "jpg",
                                "gif" => "gif",
                                _ => "png",
                            };
                            let (w, h) = image_dimensions(&bytes).unwrap_or((600, 400));
                            items.push(PdfBlock::Image(PdfImageBlock { alt, bytes, kind: kind_s, w_px: w, h_px: h }));
                        }
                        None => {
                            warnings.push(format!("Image not found, kept as text: {src}"));
                            push(&mut items, format!("[image: {alt}]"), &theme.body, false);
                        }
                    }
                }
            }
        }
    }
    let dims = theme.page_dims();
    let margins = [
        theme.page_margin(0).unwrap_or(56.0),
        theme.page_margin(1).unwrap_or(56.0),
        theme.page_margin(2).unwrap_or(56.0),
        theme.page_margin(3).unwrap_or(56.0),
    ];
    let pages = layout_pdf_pages(items, dims, margins);
    let (resolver, mut w0) = FontResolver::new(attached_fonts);
    warnings.append(&mut w0);
    let (plan_opt, mut w2) = plan_pdf_fonts(&pages, &resolver);
    warnings.append(&mut w2);
    match plan_opt {
        Some(plan) => match render_pdf_krilla(&pages, &plan, dims, margins) {
            Some(bytes) => (bytes, warnings),
            None => {
                warnings.push("PDF render failed".to_string());
                (Vec::new(), warnings)
            }
        },
        None => {
            warnings.push("No fonts available for PDF".to_string());
            (Vec::new(), warnings)
        }
    }
}

// ---------------------------------------------------------------------------
// Style files + stems + conflict-safe paths (mirrors Ariadne)
// ---------------------------------------------------------------------------

pub fn styles_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("styles")
}

pub fn list_styles(data_dir: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let dir = styles_dir(data_dir);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("css") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if !is_reserved_style(stem) && !names.contains(&stem.to_string()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
    }
    names.sort();
    names
}

pub fn sanitize_style_name(name: &str) -> String {
    let mut out: String = name.trim().chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_').collect();
    while out.ends_with(['.', ' ']) {
        out.pop();
    }
    if out.chars().count() > 60 {
        out = out.chars().take(60).collect();
    }
    out
}

/// Unused file path for a style name, appending " 2", " 3", … on collision.
/// Never overwrites — matches Ariadne's conflict handling.
pub fn style_path_for(dir: &Path, name: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{name}.css"));
    let mut n = 2;
    while candidate.exists() {
        candidate = dir.join(format!("{name} {n}.css"));
        n += 1;
    }
    candidate
}

pub fn all_style_names(data_dir: &Path, active: &str) -> Vec<String> {
    let mut styles = vec!["Built-in".to_string()];
    styles.extend(VIRTUAL_STYLES.iter().map(|(n, _)| n.to_string()));
    styles.extend(list_styles(data_dir));
    if !styles.contains(&active.to_string()) {
        styles.push(active.to_string());
    }
    styles
}

pub fn css_for_style(data_dir: &Path, name: &str) -> (String, String) {
    match name {
        "Built-in" => (String::new(), "Built-in".to_string()),
        n if virtual_style_css(n).is_some() => (virtual_style_css(n).unwrap_or_default().to_string(), n.to_string()),
        n => match std::fs::read_to_string(styles_dir(data_dir).join(format!("{n}.css"))) {
            Ok(css) => (css, n.to_string()),
            Err(_) => (String::new(), "Built-in".to_string()),
        },
    }
}

pub fn export_stem_for_titles(titles: &[String]) -> String {
    let first = titles.first().cloned().unwrap_or_else(|| "untitled".to_string());
    let mut stem: String = first.chars().map(|c| {
        if c.is_alphanumeric() || c == '-' || c == '_' {
            c
        } else if c.is_whitespace() {
            '-'
        } else {
            '\0'
        }
    }).filter(|c| *c != '\0').collect();
    while stem.contains("--") {
        stem = stem.replace("--", "-");
    }
    stem = stem.trim_matches('-').to_string();
    if stem.is_empty() {
        stem = "alcott-export".to_string();
    }
    if stem.chars().count() > 40 {
        stem = stem.chars().take(40).collect();
    }
    if titles.len() > 1 {
        stem = format!("{stem}-plus-{}", titles.len() - 1);
    }
    stem
}

/// Conflict-safe output path: when `path` exists and `force` is false,
/// append " 2", " 3", … instead of overwriting (same rule as styles).
pub fn unique_output_path(path: &Path, force: bool) -> PathBuf {
    if force || !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("export");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
    let mut n = 2;
    loop {
        let candidate = if ext.is_empty() {
            parent.join(format!("{stem} {n}"))
        } else {
            parent.join(format!("{stem} {n}.{ext}"))
        };
        if !candidate.exists() {
            return candidate;
        }
        n += 1;
    }
}

pub fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f32 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f32 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_parses_body_and_headings() {
        let css = "body { color: #ff0000; font-size: 14pt; line-height: 2; text-align: center; }\nh1 { font-size: 20pt; font-weight: bold; }\n@page { size: A4; margin: 2cm; }";
        let t = ExportTheme::from_css(css);
        assert_eq!(t.body.color, Some((255, 0, 0)));
        assert_eq!(t.body.size_pt, 14.0);
        assert_eq!(t.body.line_height, Some(2.0));
        assert_eq!(t.body.align, ExportAlign::Center);
        assert_eq!(t.h1.size_pt, 20.0);
        assert!(t.h1.bold);
        assert!(t.page.is_some());
    }

    #[test]
    fn raw_css_embeds_verbatim() {
        let css = "p { letter-spacing: 5px; }";
        let t = ExportTheme::from_css(css);
        assert_eq!(t.raw_css, css);
        // Unknown props don't affect the fixed-format subset.
        assert_eq!(t.body.size_pt, 11.0);
    }

    #[test]
    fn style_collision_appends_number() {
        let dir = std::env::temp_dir().join(format!("alcott-style-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("My Style.css"), "body {}").unwrap();
        let p = style_path_for(&dir, "My Style");
        assert_eq!(p.file_name().unwrap().to_str().unwrap(), "My Style 2.css");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn output_collision_appends_number() {
        let dir = std::env::temp_dir().join(format!("alcott-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = dir.join("doc.pdf");
        std::fs::write(&base, b"x").unwrap();
        let u = unique_output_path(&base, false);
        assert_eq!(u.file_name().unwrap().to_str().unwrap(), "doc 2.pdf");
        assert_eq!(unique_output_path(&base, true), base);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stem_sanitizes() {
        let s = export_stem_for_titles(&["Hello, World!".to_string()]);
        assert_eq!(s, "Hello-World");
        let s = export_stem_for_titles(&["A".to_string(), "B".to_string()]);
        assert!(s.ends_with("-plus-1"));
    }

    #[test]
    fn html_embeds_raw_css() {
        let docs = vec![SheetDoc::new("T".to_string(), "# Hi\n\nhello".to_string(), PathBuf::from("/tmp"))];
        let html = export_html(&docs, "p { letter-spacing: 9px; }", "stem");
        assert!(html.contains("letter-spacing: 9px"));
        assert!(html.contains("Hi"));
    }

    #[test]
    fn rtf_has_header_and_color() {
        let docs = vec![SheetDoc::new("T".to_string(), "# Hi".to_string(), PathBuf::from("/tmp"))];
        let (rtf, _) = export_rtf(&docs, "body { color: #ff0000; }", &[]);
        assert!(rtf.starts_with("{\\rtf1"));
        assert!(rtf.contains("ff0000") || rtf.contains("\\red255"));
    }

    #[test]
    fn zip_roundtrip_markers() {
        let z = build_zip(&[("a.txt", b"hi".as_slice())]);
        assert!(z.starts_with(&[0x50, 0x4B, 0x03, 0x04]));
    }

    #[test]
    fn docx_is_zip() {
        let docs = vec![SheetDoc::new("T".to_string(), "# Hi\n\npara".to_string(), PathBuf::from("/tmp"))];
        let (b, _) = export_docx_bytes(&docs, "", &[]);
        assert!(b.starts_with(&[0x50, 0x4B, 0x03, 0x04]));
    }

    #[test]
    fn epub_is_zip() {
        let docs = vec![SheetDoc::new("T".to_string(), "# Hi".to_string(), PathBuf::from("/tmp"))];
        let b = export_epub_bytes(&docs, "", "stem");
        assert!(b.starts_with(&[0x50, 0x4B, 0x03, 0x04]));
    }

    #[test]
    fn font_stack_keeps_author_order() {
        let css = "body { font-family: \"Helvetica Neue\", Helvetica, Arial, sans-serif; }";
        let t = ExportTheme::from_css(css);
        assert_eq!(
            t.body.family_stack,
            vec!["Helvetica Neue", "Helvetica", "Arial", "sans-serif"]
        );
        assert_eq!(t.body.family.as_deref(), Some("Helvetica Neue"));
    }

    #[test]
    fn font_stack_skips_css_wide_keywords() {
        let t = ExportTheme::from_css("body { font-family: inherit; }");
        assert!(t.body.family_stack.is_empty());
        assert!(t.body.family.is_none());
    }

    #[test]
    fn builtin_styles_use_linux_fonts() {
        for name in ["Manuscript", "Student", "Paperback Novel", "Professional"] {
            let css = virtual_style_css(name).expect("virtual style");
            let t = ExportTheme::from_css(css);
            let stack = t.body.family_stack.join("|").to_lowercase();
            assert!(
                stack.contains("liberation") || stack.contains("dejavu") || stack.contains("noto"),
                "{name} body stack should include a Linux-prevalent font: {stack}"
            );
        }
    }

    #[test]
    fn missing_font_warning_names_install_packages() {
        // Empty DB: nothing installed, so display falls back with a warning.
        let db = fontdb::Database::new();
        let resolver = FontResolver { attached: HashMap::new(), db };
        let (name, warn) = resolver.display(&["Helvetica Neue".to_string()], false, false);
        assert_eq!(name, "Helvetica Neue");
        let warn = warn.expect("missing font warns");
        assert!(warn.contains("not installed"), "{warn}");
        assert!(warn.contains("fonts-liberation2"), "{warn}");
        assert!(warn.contains("liberation-sans-fonts"), "{warn}");
        assert!(warn.contains("ttf-liberation"), "{warn}");
        assert!(warn.contains("liberation_ttf"), "{warn}");
    }

    #[test]
    fn substitution_warning_fires_every_export() {
        // Same stack warns each time (no cross-export suppression).
        let stack = vec!["No Such Font XYZ".to_string(), "serif".to_string()];
        let w1 = substitution_warning(&stack, "DejaVu Serif");
        let w2 = substitution_warning(&stack, "DejaVu Serif");
        assert!(w1.is_some() && w2.is_some());
        assert_eq!(w1, w2);
        // No warning when the winner is the author's first choice.
        assert!(substitution_warning(&stack, "No Such Font XYZ").is_none());
        // Generic-only stacks stay silent.
        assert!(substitution_warning(&["serif".to_string()], "DejaVu Serif").is_none());
    }

    #[test]
    fn used_slots_gate_warnings() {
        let docs = vec![SheetDoc::new("T".to_string(), "just a paragraph".to_string(), PathBuf::from("/tmp"))];
        let used = used_theme_slots(&docs);
        assert_eq!(used, [true, true, false, false, false, false, false]);
        let docs = vec![SheetDoc::new("T".to_string(), "# H\n\n> q\n\n```\nx\n```\n".to_string(), PathBuf::from("/tmp"))];
        let used = used_theme_slots(&docs);
        // Leading h1 matching the inferred title is deduped (title slot covers it).
        assert_eq!(used, [true, true, false, false, false, true, true]);
    }

    #[test]
    fn rtf_warns_on_missing_body_font() {
        // Forced empty stack choice: requested font absent everywhere.
        let docs = vec![SheetDoc::new("T".to_string(), "hi".to_string(), PathBuf::from("/tmp"))];
        let (rtf, warnings) = export_rtf(&docs, "body { font-family: \"No Such Font XYZ\", serif; }", &[]);
        assert!(rtf.starts_with("{\\rtf1"));
        assert!(
            warnings.iter().any(|w| w.contains("No Such Font XYZ")),
            "{warnings:?}"
        );
    }

    #[test]
    fn docx_warns_on_missing_body_font() {
        let docs = vec![SheetDoc::new("T".to_string(), "hi".to_string(), PathBuf::from("/tmp"))];
        let (b, warnings) = export_docx_bytes(&docs, "body { font-family: \"No Such Font XYZ\", serif; }", &[]);
        assert!(b.starts_with(&[0x50, 0x4B, 0x03, 0x04]));
        assert!(
            warnings.iter().any(|w| w.contains("No Such Font XYZ")),
            "{warnings:?}"
        );
    }
}
