use crate::util;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub mirror_repo: String,
    pub mirror_branch: String,
    pub mirror_packages_dir: String,
    pub mirror_mode: Option<String>,
    pub install_dir: String,
    pub cache_dir: String,
    pub state_file: String,
}

impl Config {
    pub fn load_or_create() -> Result<(Self, PathBuf)> {
        let path = config_path()?;
        if path.exists() {
            let content = fs::read_to_string(&path)
                .with_context(|| format!("failed to read config at {}", path.display()))?;
            let config: Config = toml::from_str(&content)
                .with_context(|| format!("failed to parse config at {}", path.display()))?;
            return Ok((config, path));
        }

        let config = Config::default_for_env()?;
        if let Some(parent) = path.parent() {
            util::ensure_dir(parent)?;
        }
        let encoded = toml::to_string_pretty(&config)?;
        fs::write(&path, encoded)
            .with_context(|| format!("failed to write config at {}", path.display()))?;
        Ok((config, path))
    }

    pub fn default_for_env() -> Result<Self> {
        let is_root = util::is_root();
        let install_dir = if let Ok(path) = env::var("TAJ_BIN_DIR") {
            PathBuf::from(path)
        } else if is_root {
            PathBuf::from("/usr/local/bin")
        } else {
            let home = env::var_os("HOME")
                .map(PathBuf::from)
                .with_context(|| "HOME is not set")?;
            home.join(".local/bin")
        };

        let cache_dir = xdg_dir("XDG_CACHE_HOME", ".cache")?.join("taj");

        let state_file = if let Ok(path) = env::var("TAJ_STATE") {
            PathBuf::from(path)
        } else if is_root {
            PathBuf::from("/var/lib/taj/installed.json")
        } else {
            xdg_dir("XDG_STATE_HOME", ".local/state")?
                .join("taj")
                .join("installed.json")
        };

        Ok(Config {
            mirror_repo: "https://github.com/taj-pm/packages".to_string(),
            mirror_branch: "main".to_string(),
            mirror_packages_dir: "packages".to_string(),
            mirror_mode: Some("git".to_string()),
            install_dir: install_dir.to_string_lossy().to_string(),
            cache_dir: cache_dir.to_string_lossy().to_string(),
            state_file: state_file.to_string_lossy().to_string(),
        })
    }
}

fn config_path() -> Result<PathBuf> {
    if let Ok(path) = env::var("TAJ_CONFIG") {
        return Ok(PathBuf::from(path));
    }

    if util::is_root() {
        return Ok(PathBuf::from("/etc/taj/config.toml"));
    }

    Ok(xdg_dir("XDG_CONFIG_HOME", ".config")?
        .join("taj")
        .join("config.toml"))
}

fn xdg_dir(var: &str, fallback: &str) -> Result<PathBuf> {
    if let Some(dir) = env::var_os(var) {
        return Ok(PathBuf::from(dir));
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .with_context(|| format!("{} is not set and HOME is missing", var))?;
    Ok(home.join(fallback))
}

pub fn install_dir(config: &Config) -> Result<PathBuf> {
    Ok(Path::new(&config.install_dir).to_path_buf())
}

pub fn cache_dir(config: &Config) -> Result<PathBuf> {
    Ok(Path::new(&config.cache_dir).to_path_buf())
}

pub fn state_file(config: &Config) -> Result<PathBuf> {
    Ok(Path::new(&config.state_file).to_path_buf())
}
