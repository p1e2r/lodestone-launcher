use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::paths::paths;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub max_memory_mb: u32,
    pub min_memory_mb: u32,
    pub java_override: Option<PathBuf>,
    pub extra_jvm_args: String,
    pub show_snapshots: bool,
    pub show_old: bool,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub hide_on_launch: bool,
    pub check_updates: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let total_mb = total_memory_mb();
        // A quarter of RAM, between 2 and 8 GB.
        let max = (total_mb / 4).clamp(2048, 8192);
        Settings {
            max_memory_mb: max,
            min_memory_mb: 512,
            java_override: None,
            extra_jvm_args: String::new(),
            show_snapshots: false,
            show_old: false,
            width: None,
            height: None,
            hide_on_launch: false,
            check_updates: true,
        }
    }
}

impl Settings {
    pub fn load() -> Settings {
        std::fs::read(paths().settings_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self) {
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(paths().settings_file(), b);
        }
    }
}

pub fn total_memory_mb() -> u32 {
    static TOTAL: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *TOTAL.get_or_init(|| match sysinfo::System::new() {
        Ok(mut sys) => {
            sys.refresh_memory();
            (sys.total_memory() / 1024 / 1024) as u32
        }
        Err(_) => 8192,
    })
}

/// Split extra JVM args, respecting simple double quotes.
pub fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}
