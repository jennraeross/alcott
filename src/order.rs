//! Manual sheet ordering.
//!
//! Stored at `<library>/.alcott/order.json` (hidden, so it never shows up as
//! a group and search skips it):
//! `{"v":1,"order":{"Writing/Book 1":["b.md","a.md"]}}` — per-group filename
//! arrays. Listed names come first in listed order; anything unlisted (new
//! or externally added files) appends alphabetically after. Stale entries
//! are pruned on load.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const ORDER_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct OrderFile {
    #[serde(default)]
    pub v: u32,
    #[serde(default)]
    pub order: HashMap<String, Vec<String>>,
}

impl OrderFile {
    pub fn path_for(root: &Path) -> PathBuf {
        root.join(".alcott").join("order.json")
    }

    pub fn load(root: &Path) -> Self {
        let bytes = std::fs::read(Self::path_for(root));
        match bytes {
            Ok(b) => serde_json::from_slice(&b).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, root: &Path) {
        let p = Self::path_for(root);
        if p.parent().map(|d| std::fs::create_dir_all(d).is_ok()) != Some(true) {
            return;
        }
        let mut with_version = self.clone();
        with_version.v = ORDER_VERSION;
        if let Ok(s) = serde_json::to_string_pretty(&with_version) {
            let _ = std::fs::write(p, s);
        }
    }

    /// Drop keys for missing groups and names for missing files (plus dupes).
    pub fn prune(
        &mut self,
        valid_groups: &HashSet<String>,
        valid_files: &HashSet<(String, String)>,
    ) {
        self.order.retain(|g, _| valid_groups.contains(g));
        for (g, names) in self.order.iter_mut() {
            let mut seen = HashSet::new();
            names.retain(|n| {
                valid_files.contains(&(g.clone(), n.clone())) && seen.insert(n.clone())
            });
        }
        self.order.retain(|_, names| !names.is_empty());
    }

    /// Display order for a group: listed names first, then unlisted sorted.
    pub fn display_order(&self, group: &str, existing: &[String]) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(list) = self.order.get(group) {
            for n in list {
                if existing.iter().any(|e| e == n) {
                    out.push(n.clone());
                }
            }
        }
        let mut rest: Vec<String> = existing
            .iter()
            .filter(|n| !out.iter().any(|x| x == *n))
            .cloned()
            .collect();
        rest.sort();
        out.extend(rest);
        out
    }

    pub fn rank(&self, group: &str, name: &str) -> Option<usize> {
        self.order.get(group)?.iter().position(|n| n == name)
    }

    pub fn append(&mut self, group: &str, name: &str) {
        let list = self.order.entry(group.to_string()).or_default();
        if !list.iter().any(|n| n == name) {
            list.push(name.to_string());
        }
    }

    pub fn remove(&mut self, group: &str, name: &str) {
        if let Some(list) = self.order.get_mut(group) {
            list.retain(|n| n != name);
            if list.is_empty() {
                self.order.remove(group);
            }
        }
    }

    pub fn rename(&mut self, group: &str, old: &str, new: &str) {
        match self.order.get_mut(group) {
            Some(list) => match list.iter().position(|n| n == old) {
                Some(pos) => list[pos] = new.to_string(),
                None => list.push(new.to_string()),
            },
            None => {
                self.order.insert(group.to_string(), vec![new.to_string()]);
            }
        }
    }

    /// Replace a group's full explicit order (used by reorder/drop).
    pub fn set_order(&mut self, group: &str, names: Vec<String>) {
        if names.is_empty() {
            self.order.remove(group);
        } else {
            self.order.insert(group.to_string(), names);
        }
    }

    /// Swap a name with its neighbor in display order (+1 down, -1 up).
    /// Returns the new index, or None if the name isn't present.
    pub fn swap_neighbor(
        &mut self,
        group: &str,
        existing: &[String],
        name: &str,
        dir: i32,
    ) -> Option<usize> {
        let mut disp = self.display_order(group, existing);
        let i = disp.iter().position(|n| n == name)?;
        let j = (i as i32 + dir).clamp(0, disp.len() as i32 - 1) as usize;
        if i == j {
            return Some(i);
        }
        disp.swap(i, j);
        self.set_order(group, disp);
        Some(j)
    }

    /// Delete a group key and every descendant key (`G` and `G/...`).
    pub fn remove_group_prefix(&mut self, prefix: &str) {
        let with_sep = format!("{prefix}/");
        self.order
            .retain(|k, _| k != prefix && !k.starts_with(&with_sep));
    }

    /// Rewrite group keys when a group moves (`G[/...]` → `N[/...]`).
    pub fn rename_group_prefix(&mut self, old: &str, new: &str) {
        let with_sep = format!("{old}/");
        let mut rewritten = HashMap::new();
        for (k, v) in self.order.drain() {
            if k == old {
                rewritten.insert(new.to_string(), v);
            } else if let Some(rest) = k.strip_prefix(&with_sep) {
                rewritten.insert(format!("{new}/{rest}"), v);
            } else {
                rewritten.insert(k, v);
            }
        }
        self.order = rewritten;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_first_then_alpha() {
        let mut o = OrderFile::default();
        o.order.insert("g".into(), vec!["b.md".into(), "stale.md".into()]);
        let disp = o.display_order("g", &["c.md".to_string(), "a.md".to_string(), "b.md".to_string()]);
        assert_eq!(disp, vec!["b.md", "a.md", "c.md"]);
    }

    #[test]
    fn prune_drops_stale() {
        let mut o = OrderFile::default();
        o.order.insert("g".into(), vec!["b.md".into(), "gone.md".into()]);
        o.order.insert("dead".into(), vec!["x.md".into()]);
        let groups: HashSet<String> = ["g".to_string()].into();
        let files: HashSet<(String, String)> = [("g".to_string(), "b.md".to_string())].into();
        o.prune(&groups, &files);
        assert_eq!(o.order.len(), 1);
        assert_eq!(o.order["g"], vec!["b.md"]);
    }

    #[test]
    fn swap_and_group_prefix() {
        let mut o = OrderFile::default();
        o.order.insert("g".into(), vec!["a".into(), "b".into(), "c".into()]);
        let existing = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(o.swap_neighbor("g", &existing, "a", 1), Some(1));
        assert_eq!(o.order["g"], vec!["b", "a", "c"]);
        o.swap_neighbor("g", &existing, "b", -1);
        assert_eq!(o.order["g"][0], "b");
        o.rename_group_prefix("g", "h");
        assert!(o.order.contains_key("h"));
        o.remove_group_prefix("h");
        assert!(o.order.is_empty());
    }
}
