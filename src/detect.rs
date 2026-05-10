use crate::build::{cargo_bin_name, BuildMethod, BuildSpec};
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;

pub fn detect_build_spec(repo_dir: &Path, name_hint: &str) -> Result<BuildSpec> {
    let empty_env = HashMap::new();

    let cargo_toml = repo_dir.join("Cargo.toml");
    if cargo_toml.exists() {
        let bin = cargo_bin_name(&cargo_toml)?.or_else(|| Some(name_hint.to_string()));
        return Ok(BuildSpec {
            method: BuildMethod::Cargo,
            args: Vec::new(),
            bin,
            subdir: None,
            env: empty_env,
        });
    }

    if repo_dir.join("go.mod").exists() {
        return Ok(BuildSpec {
            method: BuildMethod::Go,
            args: Vec::new(),
            bin: Some(name_hint.to_string()),
            subdir: None,
            env: empty_env,
        });
    }

    if repo_dir.join("CMakeLists.txt").exists() {
        return Ok(BuildSpec {
            method: BuildMethod::Cmake,
            args: Vec::new(),
            bin: None,
            subdir: None,
            env: empty_env,
        });
    }

    if repo_dir.join("Makefile").exists() || repo_dir.join("makefile").exists() {
        return Ok(BuildSpec {
            method: BuildMethod::Make,
            args: Vec::new(),
            bin: None,
            subdir: None,
            env: empty_env,
        });
    }

    if has_extension(repo_dir, &["cpp", "cc", "cxx"]) {
        return Ok(BuildSpec {
            method: BuildMethod::Gpp,
            args: Vec::new(),
            bin: Some(name_hint.to_string()),
            subdir: None,
            env: empty_env,
        });
    }

    if has_extension(repo_dir, &["c"]) {
        return Ok(BuildSpec {
            method: BuildMethod::Gcc,
            args: Vec::new(),
            bin: Some(name_hint.to_string()),
            subdir: None,
            env: empty_env,
        });
    }

    if repo_dir.join("src").join("main.rs").exists() || repo_dir.join("main.rs").exists() {
        return Ok(BuildSpec {
            method: BuildMethod::Rustc,
            args: Vec::new(),
            bin: Some(name_hint.to_string()),
            subdir: None,
            env: empty_env,
        });
    }

    bail!("could not detect a build system in {}", repo_dir.display())
}

fn has_extension(repo_dir: &Path, extensions: &[&str]) -> bool {
    let entries = match std::fs::read_dir(repo_dir) {
        Ok(entries) => entries,
        Err(_) => return false,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.iter().any(|allowed| allowed.eq_ignore_ascii_case(ext)) {
                    return true;
                }
            }
        }
    }
    false
}
