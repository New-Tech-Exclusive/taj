use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallRecord {
    pub name: String,
    pub bin_name: String,
    pub bin_path: String,
    pub repo: String,
    pub source: String,
    pub installed_at: i64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct State {
    pub packages: HashMap<String, InstallRecord>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(State::default());
        }
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read state at {}", path.display()))?;
        let state: State = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse state at {}", path.display()))?;
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let encoded = serde_json::to_string_pretty(self)?;
        fs::write(path, encoded)
            .with_context(|| format!("failed to write state at {}", path.display()))?;
        Ok(())
    }
}
