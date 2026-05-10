use crate::build::{BuildMethod, BuildSpec};
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct TajFile {
    pub name: Option<String>,
    pub repo: String,
    pub build: String,
    pub build_args: Option<Vec<String>>,
    pub bin: Option<String>,
    pub subdir: Option<String>,
    pub env: Option<HashMap<String, String>>,
}

impl TajFile {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read taj file at {}", path.display()))?;
        let taj: TajFile = toml::from_str(&content)
            .with_context(|| format!("failed to parse taj file at {}", path.display()))?;
        Ok(taj)
    }

    pub fn to_build_spec(&self) -> Result<BuildSpec> {
        let method: BuildMethod = self.build.parse()?;
        Ok(BuildSpec {
            method,
            args: self.build_args.clone().unwrap_or_default(),
            bin: self.bin.clone(),
            subdir: self.subdir.clone().map(|dir| dir.into()),
            env: self.env.clone().unwrap_or_default(),
        })
    }
}
