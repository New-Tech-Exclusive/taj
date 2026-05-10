use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn is_root() -> bool {
    #[cfg(unix)]
    {
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub fn now_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path).with_context(|| format!("failed to create {}", path.display()))
}

pub fn require_tool(name: &str) -> Result<PathBuf> {
    which::which(name).with_context(|| format!("required tool '{}' not found in PATH", name))
}

pub fn run_command(
    cmd: &str,
    args: &[String],
    cwd: Option<&Path>,
    envs: Option<&HashMap<String, String>>,
) -> Result<()> {
    let mut command = Command::new(cmd);
    command.args(args);
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    if let Some(envs) = envs {
        for (key, value) in envs {
            command.env(key, value);
        }
    }
    let status = command
        .status()
        .with_context(|| format!("failed to run {}", cmd))?;
    if !status.success() {
        bail!("command failed: {} {:?}", cmd, args);
    }
    Ok(())
}

pub fn repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches(".git");
    let normalized = trimmed.replace(':', "/");
    normalized
        .split('/')
        .last()
        .unwrap_or("package")
        .to_string()
}

pub fn path_in_env(path: &Path) -> bool {
    if let Ok(path_var) = env::var("PATH") {
        for entry in env::split_paths(&path_var) {
            if entry == path {
                return true;
            }
        }
    }
    false
}
