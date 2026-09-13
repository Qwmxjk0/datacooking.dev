use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MAX_NAME: usize = 24;
const MAX_NOTE: usize = 80;
const MAX_QUEUE: usize = 80;
const RATE_WINDOW: Duration = Duration::from_secs(60);
const RATE_MAX: usize = 20;

#[derive(Clone, Serialize, Deserialize)]
pub struct Donor {
    pub name: String,
    pub note: String,
}

pub struct DonorQueue {
    path: PathBuf,
    hits: Mutex<Vec<Instant>>,
}

impl DonorQueue {
    pub fn new(path: PathBuf) -> Self {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if !path.exists() {
            let _ = std::fs::write(&path, "[]");
        }
        Self {
            path,
            hits: Mutex::new(Vec::new()),
        }
    }

    pub fn list(&self) -> Vec<Donor> {
        read_all(&self.path)
    }

    pub fn push(&self, name: &str, note: &str) -> Result<Donor, String> {
        if !self.allow() {
            return Err("รอสักครู่แล้วค่อยส่งใหม่".into());
        }
        let name = clean(name, MAX_NAME);
        let note = clean(note, MAX_NOTE);
        if name.is_empty() {
            return Err("ใส่ชื่อเล่นสั้นๆ".into());
        }
        let donor = Donor { name, note };
        let mut all = read_all(&self.path);
        all.push(donor.clone());
        if all.len() > MAX_QUEUE {
            all = all.split_off(all.len() - MAX_QUEUE);
        }
        let json = serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, json).map_err(|e| e.to_string())?;
        Ok(donor)
    }

    fn allow(&self) -> bool {
        let Ok(mut hits) = self.hits.lock() else {
            return false;
        };
        let now = Instant::now();
        hits.retain(|t| now.duration_since(*t) < RATE_WINDOW);
        if hits.len() >= RATE_MAX {
            return false;
        }
        hits.push(now);
        true
    }
}

fn read_all(path: &Path) -> Vec<Donor> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn clean(input: &str, max: usize) -> String {
    input
        .chars()
        .filter(|c| !c.is_control())
        .filter(|c| *c != '<' && *c != '>' && *c != '&')
        .take(max)
        .collect::<String>()
        .trim()
        .to_string()
}
