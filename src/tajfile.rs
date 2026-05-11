use crate::build::{BuildMethod, BuildSpec};
use anyhow::{Context, Result};
use serde::de::Deserializer;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct TajFile {
    pub name: Option<String>,
    pub category: Option<String>,
    pub repo: String,
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
    pub build: String,
    #[serde(default, deserialize_with = "deserialize_build_args")]
    pub build_args: Vec<String>,
    pub build_dir: Option<String>,
    pub bin: Option<String>,
    pub subdir: Option<String>,
    pub env: Option<HashMap<String, String>>,
    pub deps: Option<TajDeps>,
    pub depends: Option<Vec<String>>,
    pub install: Option<InstallSpec>,
}

#[derive(Debug, Deserialize)]
pub struct TajDeps {
    pub tools: Option<Vec<String>>,
    pub pkg_config: Option<Vec<String>>,
    pub packages: Option<HashMap<String, Vec<String>>>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct InstallSpec {
    pub method: String,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub prefix: Option<String>,
    pub destdir: Option<String>,
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
            args: self.build_args.clone(),
            bin: self.bin.clone(),
            subdir: self.subdir.clone().map(|dir| dir.into()),
            build_dir: self.build_dir.clone().map(|dir| dir.into()),
            env: self.env.clone().unwrap_or_default(),
        })
    }
}

fn deserialize_build_args<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrVec {
        Single(String),
        Multiple(Vec<String>),
    }

    match StringOrVec::deserialize(deserializer)? {
        StringOrVec::Single(value) => Ok(vec![value]),
        StringOrVec::Multiple(values) => Ok(values),
    }
}
