use std::path::Path;

use chrono::Local;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Daily {
    pub date: String, // YYYY-MM-DD
    pub baseline_total: usize,
    pub gross_typed: usize,
}

impl Daily {
    pub fn today() -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    fn path(data_dir: &Path) -> std::path::PathBuf {
        data_dir.join("daily.json")
    }

    pub fn load(data_dir: &Path, current_total: usize) -> Self {
        let p = Self::path(data_dir);
        if let Ok(bytes) = std::fs::read(&p) {
            if let Ok(mut d) = serde_json::from_slice::<Daily>(&bytes) {
                if d.date == Self::today() {
                    return d;
                }
                // rollover: new day, baseline = current total
                d = Daily {
                    date: Self::today(),
                    baseline_total: current_total,
                    gross_typed: 0,
                };
                let _ = d.save(data_dir);
                return d;
            }
        }
        let d = Daily {
            date: Self::today(),
            baseline_total: current_total,
            gross_typed: 0,
        };
        let _ = d.save(data_dir);
        d
    }

    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        std::fs::write(Self::path(data_dir), serde_json::to_string_pretty(self).unwrap())?;
        Ok(())
    }

    pub fn record_words_completed(&mut self, n: usize, data_dir: &Path) {
        self.gross_typed += n;
        let _ = self.save(data_dir);
    }

    /// Net words added today (can be negative after deletions).
    pub fn net(&self, current_total: usize) -> isize {
        current_total as isize - self.baseline_total as isize
    }
}
