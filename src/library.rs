use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::order::OrderFile;
use crate::words;

pub fn group_key(rel: &Path) -> String {
    rel.to_string_lossy().to_string()
}

#[derive(Debug, Clone)]
pub struct SheetMeta {
    pub abs: PathBuf,
    /// relative to library root, e.g. Writing/Book 1/Sheet3.md
    pub rel: PathBuf,
    pub name: String,
    pub group: PathBuf, // parent dir rel
    pub preview: String,
    pub words: usize,
    #[allow(dead_code)]
    pub mtime: Option<std::time::SystemTime>,
}

#[derive(Debug, Clone)]
pub struct FlatGroup {
    pub rel: PathBuf,
    pub name: String,
    pub depth: usize,
}

#[derive(Debug)]
pub struct Library {
    #[allow(dead_code)]
    pub root: PathBuf,
    pub groups: Vec<FlatGroup>, // flattened, depth-ordered
    pub collapsed: HashSet<PathBuf>,
    pub sheets: Vec<SheetMeta>,
    pub word_cache: HashMap<PathBuf, usize>, // abs -> words
    pub total_words: usize,
    pub order: OrderFile,
}

fn slug_filename(title: &str) -> String {
    let t = title.trim();
    if t.is_empty() {
        return "Untitled.md".to_string();
    }
    let mut s = t.to_string();
    if !s.to_lowercase().ends_with(".md") {
        s.push_str(".md");
    }
    s
}

pub fn slugify(title: &str) -> String {
    slug_filename(title)
}

fn rank_in(order: &OrderFile, s: &SheetMeta) -> usize {
    order
        .rank(&group_key(&s.group), &s.name)
        .unwrap_or(usize::MAX)
}

fn read_preview_and_words(abs: &Path) -> (String, usize) {
    match std::fs::read_to_string(abs) {
        Ok(text) => (words::sheet_preview(&text), words::count_words(&text)),
        Err(_) => ("preview of the\ntext...".to_string(), 0),
    }
}

fn collect_groups(root: &Path, out: &mut Vec<FlatGroup>, rel: &Path, depth: usize) {
    let abs = if rel.as_os_str().is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    let Ok(entries) = std::fs::read_dir(&abs) else {
        return;
    };
    let mut dirs: Vec<String> = Vec::new();
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                if !name.starts_with('.') {
                    dirs.push(name.to_string());
                }
            }
        }
    }
    dirs.sort();
    for name in dirs {
        let child_rel = if rel.as_os_str().is_empty() {
            PathBuf::from(&name)
        } else {
            rel.join(&name)
        };
        out.push(FlatGroup {
            rel: child_rel.clone(),
            name,
            depth,
        });
        collect_groups(root, out, &child_rel, depth + 1);
    }
}

fn collect_sheets(root: &Path, out: &mut Vec<SheetMeta>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs: Vec<PathBuf> = Vec::new();
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(n) = p.file_name().and_then(|s| s.to_str()) {
                    if n.starts_with('.') {
                        continue;
                    }
                }
                subdirs.push(p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
                let rel = p.strip_prefix(root).unwrap_or(&p).to_path_buf();
                let group = rel.parent().map(|g| g.to_path_buf()).unwrap_or_default();
                let name = p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?.md")
                    .to_string();
                let mtime = std::fs::metadata(&p).and_then(|m| m.modified()).ok();
                let (preview, wc) = read_preview_and_words(&p);
                out.push(SheetMeta {
                    abs: p,
                    rel,
                    name,
                    group,
                    preview,
                    words: wc,
                    mtime,
                });
            }
        }
        subdirs.sort();
        for d in subdirs {
            stack.push(d);
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
}

impl Library {
    pub fn scan(root: &Path) -> Self {
        let mut groups = Vec::new();
        collect_groups(root, &mut groups, Path::new(""), 0);
        let mut sheets = Vec::new();
        if root.exists() {
            collect_sheets(root, &mut sheets);
        }
        // Manual order: listed names first, unlisted alphabetical after.
        let mut order = OrderFile::load(root);
        let valid_groups: HashSet<String> = groups
            .iter()
            .map(|g| group_key(&g.rel))
            .chain(sheets.iter().map(|s| group_key(&s.group)))
            .collect();
        let valid_files: HashSet<(String, String)> = sheets
            .iter()
            .map(|s| (group_key(&s.group), s.name.clone()))
            .collect();
        order.prune(&valid_groups, &valid_files);
        sheets.sort_by(|a, b| {
            a.group
                .cmp(&b.group)
                .then_with(|| {
                    rank_in(&order, a)
                        .cmp(&rank_in(&order, b))
                })
                .then_with(|| a.name.cmp(&b.name))
        });
        let mut word_cache = HashMap::new();
        let mut total = 0;
        for s in &sheets {
            word_cache.insert(s.abs.clone(), s.words);
            total += s.words;
        }
        Self {
            root: root.to_path_buf(),
            groups,
            collapsed: HashSet::new(),
            sheets,
            word_cache,
            total_words: total,
            order,
        }
    }

    /// Persist manual order (call after every order mutation, before rescan).
    pub fn save_order(&self) {
        self.order.save(&self.root);
    }

    pub fn visible_groups(&self) -> Vec<&FlatGroup> {
        let mut out = Vec::new();
        for g in &self.groups {
            // hidden if any ancestor collapsed
            let mut hidden = false;
            let mut anc = g.rel.parent();
            while let Some(a) = anc {
                if a.as_os_str().is_empty() {
                    break;
                }
                if self.collapsed.contains(a) {
                    hidden = true;
                    break;
                }
                anc = a.parent();
            }
            if !hidden {
                out.push(g);
            }
        }
        out
    }

    pub fn has_children(&self, rel: &Path) -> bool {
        self.groups
            .iter()
            .any(|g| g.rel.parent() == Some(rel))
    }

    pub fn toggle(&mut self, rel: &Path) {
        if self.collapsed.contains(rel) {
            self.collapsed.remove(rel);
        } else if self.has_children(rel) {
            self.collapsed.insert(rel.to_path_buf());
        }
    }

    pub fn sheets_for_group(&self, group: Option<&Path>) -> Vec<&SheetMeta> {
        match group {
            None => self.sheets.iter().collect(),
            Some(g) if g.as_os_str().is_empty() => self.sheets.iter().collect(),
            Some(g) => self.sheets.iter().filter(|s| s.group == g).collect(),
        }
    }

    pub fn refresh_sheet(&mut self, abs: &Path, new_words: usize, new_preview: String) {
        if let Some(old) = self.word_cache.get(abs) {
            self.total_words = self.total_words.saturating_sub(*old);
        }
        self.word_cache.insert(abs.to_path_buf(), new_words);
        self.total_words += new_words;
        if let Some(s) = self.sheets.iter_mut().find(|s| s.abs == abs) {
            s.words = new_words;
            s.preview = new_preview;
        }
    }

    pub fn create_sheet(
        root: &Path,
        group: &str,
        title: &str,
    ) -> std::io::Result<PathBuf> {
        let fname = slugify(title);
        let dir = if group.trim().is_empty() {
            root.to_path_buf()
        } else {
            root.join(group)
        };
        std::fs::create_dir_all(&dir)?;
        let abs = dir.join(&fname);
        if abs.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("sheet already exists: {}", abs.display()),
            ));
        }
        let stem = fname.trim_end_matches(".md").trim_end_matches(".MD");
        let content = format!("# {stem}\n\n");
        std::fs::write(&abs, content)?;
        Ok(abs)
    }

    fn sanitize_group_name(name: &str) -> String {
        name.trim()
            .replace(['/', '\\'], "-")
            .trim_matches('.')
            .trim()
            .to_string()
    }

    /// Create a subgroup under `parent` (empty = library root).
    /// Returns the new group's path relative to `root`.
    pub fn create_group(root: &Path, parent: &Path, name: &str) -> std::io::Result<PathBuf> {
        let clean = Self::sanitize_group_name(name);
        if clean.is_empty() || clean == "." || clean == ".." {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "group name is empty",
            ));
        }
        let rel = if parent.as_os_str().is_empty() {
            PathBuf::from(&clean)
        } else {
            parent.join(&clean)
        };
        let dir = root.join(&rel);
        if dir.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("group already exists: {}", dir.display()),
            ));
        }
        std::fs::create_dir_all(&dir)?;
        Ok(rel)
    }

    /// Rename a group in place (same parent). Returns the new rel path.
    pub fn rename_group(root: &Path, group: &Path, new_name: &str) -> std::io::Result<PathBuf> {
        let clean = Self::sanitize_group_name(new_name);
        if clean.is_empty() || clean == "." || clean == ".." {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "group name is empty",
            ));
        }
        let parent = group.parent().unwrap_or_else(|| Path::new(""));
        let target = if parent.as_os_str().is_empty() {
            PathBuf::from(&clean)
        } else {
            parent.join(&clean)
        };
        if target == group {
            return Ok(target);
        }
        let target_abs = root.join(&target);
        if target_abs.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("group already exists: {}", target_abs.display()),
            ));
        }
        std::fs::rename(root.join(group), &target_abs)?;
        Ok(target)
    }

    /// Delete a group recursively.
    pub fn delete_group(root: &Path, group: &Path) -> std::io::Result<()> {
        if group.as_os_str().is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "cannot delete the library root",
            ));
        }
        std::fs::remove_dir_all(root.join(group))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("alcott-lib-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn group_crud_roundtrip() {
        let root = tmp_root("crud");
        let rel = Library::create_group(&root, Path::new(""), "Writing").unwrap();
        assert!(root.join(&rel).is_dir());
        assert!(Library::create_group(&root, Path::new(""), "Writing").is_err());
        let sub = Library::create_group(&root, &rel, "Book 1").unwrap();
        assert!(root.join(&sub).is_dir());
        let renamed = Library::rename_group(&root, &sub, "Book 2").unwrap();
        assert!(!root.join(&sub).exists());
        assert!(root.join(&renamed).is_dir());
        assert!(Library::rename_group(&root, &rel, "Writing").is_ok());
        Library::delete_group(&root, &renamed).unwrap();
        assert!(!root.join(&renamed).exists());
        let lib = Library::scan(&root);
        assert!(lib.groups.iter().any(|g| g.rel == rel));
        assert!(Library::delete_group(&root, Path::new("")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn group_names_are_sanitized() {
        let root = tmp_root("san");
        assert!(Library::create_group(&root, Path::new(""), "  ").is_err());
        assert!(Library::create_group(&root, Path::new(""), "a/b").is_ok());
        assert!(root.join("a-b").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }
}
