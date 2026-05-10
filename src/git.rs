use crate::config::Config;
use crate::util;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

pub fn clone_repo(url: &str, dest: &Path) -> Result<()> {
    util::require_tool("git")?;

    let args = vec![
        "clone".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        url.to_string(),
        dest.display().to_string(),
    ];
    util::run_command("git", &args, None, None)
}

pub fn ensure_mirror(config: &Config, cache_dir: &Path) -> Result<PathBuf> {
    util::require_tool("git")?;

    let mirror_dir = cache_dir.join("mirror");
    let git_dir = mirror_dir.join(".git");

    if mirror_dir.exists() && !git_dir.exists() {
        bail!(
            "mirror path exists but is not a git repo: {}",
            mirror_dir.display()
        );
    }

    if !mirror_dir.exists() {
        let args = vec![
            "clone".to_string(),
            "--depth".to_string(),
            "1".to_string(),
            "-b".to_string(),
            config.mirror_branch.clone(),
            config.mirror_repo.clone(),
            mirror_dir.display().to_string(),
        ];
        util::run_command("git", &args, None, None)?;
        return Ok(mirror_dir);
    }

    let fetch_args = vec![
        "-C".to_string(),
        mirror_dir.display().to_string(),
        "fetch".to_string(),
        "--depth".to_string(),
        "1".to_string(),
        "origin".to_string(),
        config.mirror_branch.clone(),
    ];
    util::run_command("git", &fetch_args, None, None).with_context(|| {
        format!("failed to fetch mirror at {}", mirror_dir.display())
    })?;

    let reset_args = vec![
        "-C".to_string(),
        mirror_dir.display().to_string(),
        "reset".to_string(),
        "--hard".to_string(),
        "FETCH_HEAD".to_string(),
    ];
    util::run_command("git", &reset_args, None, None).with_context(|| {
        format!("failed to reset mirror at {}", mirror_dir.display())
    })?;

    Ok(mirror_dir)
}
