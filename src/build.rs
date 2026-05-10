use crate::util;
use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::SystemTime;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub enum BuildMethod {
    Cargo,
    Go,
    Make,
    Cmake,
    Gcc,
    Gpp,
    Rustc,
}

impl FromStr for BuildMethod {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value.to_lowercase().as_str() {
            "cargo" => Ok(BuildMethod::Cargo),
            "go" => Ok(BuildMethod::Go),
            "make" => Ok(BuildMethod::Make),
            "cmake" => Ok(BuildMethod::Cmake),
            "gcc" | "c" => Ok(BuildMethod::Gcc),
            "g++" | "gpp" | "cpp" => Ok(BuildMethod::Gpp),
            "rustc" => Ok(BuildMethod::Rustc),
            _ => bail!("unknown build method: {}", value),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BuildSpec {
    pub method: BuildMethod,
    pub args: Vec<String>,
    pub bin: Option<String>,
    pub subdir: Option<PathBuf>,
    pub env: HashMap<String, String>,
}

pub fn build(repo_dir: &Path, spec: &BuildSpec, name_hint: &str, out_dir: &Path) -> Result<PathBuf> {
    util::ensure_dir(out_dir)?;

    match spec.method {
        BuildMethod::Cargo => build_cargo(repo_dir, spec, name_hint),
        BuildMethod::Go => build_go(repo_dir, spec, name_hint, out_dir),
        BuildMethod::Make => build_make(repo_dir, spec),
        BuildMethod::Cmake => build_cmake(repo_dir, spec, name_hint, out_dir),
        BuildMethod::Gcc => build_gcc(repo_dir, spec, name_hint, out_dir),
        BuildMethod::Gpp => build_gpp(repo_dir, spec, name_hint, out_dir),
        BuildMethod::Rustc => build_rustc(repo_dir, spec, name_hint, out_dir),
    }
}

pub fn cargo_bin_name(manifest_path: &Path) -> Result<Option<String>> {
    let content = fs::read_to_string(manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let value: toml::Value = toml::from_str(&content)
        .with_context(|| format!("failed to parse {}", manifest_path.display()))?;

    if let Some(bins) = value.get("bin").and_then(|v| v.as_array()) {
        if let Some(name) = bins
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .next()
        {
            return Ok(Some(name.to_string()));
        }
    }

    if let Some(pkg) = value.get("package") {
        if let Some(name) = pkg.get("name").and_then(|n| n.as_str()) {
            return Ok(Some(name.to_string()));
        }
    }

    Ok(None)
}

fn build_cargo(repo_dir: &Path, spec: &BuildSpec, name_hint: &str) -> Result<PathBuf> {
    util::require_tool("cargo")?;

    let mut args = vec!["build".to_string(), "--release".to_string()];
    args.extend(spec.args.clone());
    util::run_command("cargo", &args, Some(repo_dir), Some(&spec.env))?;

    let bin_name = if let Some(bin) = &spec.bin {
        bin.clone()
    } else {
        cargo_bin_name(&repo_dir.join("Cargo.toml"))?
            .unwrap_or_else(|| name_hint.to_string())
    };

    let bin_path = repo_dir
        .join("target")
        .join("release")
        .join(&bin_name);
    if !bin_path.exists() {
        bail!("cargo build did not produce {}", bin_path.display());
    }
    Ok(bin_path)
}

fn build_go(repo_dir: &Path, spec: &BuildSpec, name_hint: &str, out_dir: &Path) -> Result<PathBuf> {
    util::require_tool("go")?;

    let bin_name = spec.bin.clone().unwrap_or_else(|| name_hint.to_string());
    let out_path = out_dir.join(&bin_name);

    let mut args = vec!["build".to_string(), "-o".to_string(), out_path.display().to_string()];
    args.extend(spec.args.clone());
    util::run_command("go", &args, Some(repo_dir), Some(&spec.env))?;

    if !out_path.exists() {
        bail!("go build did not produce {}", out_path.display());
    }
    Ok(out_path)
}

fn build_make(repo_dir: &Path, spec: &BuildSpec) -> Result<PathBuf> {
    util::require_tool("make")?;

    let before = list_executables(repo_dir)?;

    let mut args = Vec::new();
    args.extend(spec.args.clone());
    util::run_command("make", &args, Some(repo_dir), Some(&spec.env))?;

    let after = list_executables(repo_dir)?;
    let candidates = diff_executables(&before, &after);
    pick_executable(candidates, spec.bin.as_deref())
}

fn build_cmake(
    repo_dir: &Path,
    spec: &BuildSpec,
    _name_hint: &str,
    out_dir: &Path,
) -> Result<PathBuf> {
    util::require_tool("cmake")?;

    let build_dir = repo_dir.join("build");
    util::ensure_dir(&build_dir)?;

    let mut config_args = vec![
        "-S".to_string(),
        ".".to_string(),
        "-B".to_string(),
        build_dir.display().to_string(),
        "-DCMAKE_BUILD_TYPE=Release".to_string(),
        format!("-DCMAKE_RUNTIME_OUTPUT_DIRECTORY={}", out_dir.display()),
    ];
    config_args.extend(spec.args.clone());
    util::run_command("cmake", &config_args, Some(repo_dir), Some(&spec.env))?;

    let build_args = vec![
        "--build".to_string(),
        build_dir.display().to_string(),
        "--config".to_string(),
        "Release".to_string(),
    ];
    util::run_command("cmake", &build_args, Some(repo_dir), Some(&spec.env))?;

    let candidates = list_executables(out_dir)?;
    pick_executable(candidates, spec.bin.as_deref())
}

fn build_gcc(repo_dir: &Path, spec: &BuildSpec, name_hint: &str, out_dir: &Path) -> Result<PathBuf> {
    util::require_tool("gcc")?;
    let sources = collect_sources(repo_dir, &["c"])?;
    if sources.is_empty() {
        bail!("no .c sources found for gcc build");
    }

    let bin_name = spec.bin.clone().unwrap_or_else(|| name_hint.to_string());
    let out_path = out_dir.join(&bin_name);

    let mut args = vec!["-O2".to_string(), "-o".to_string(), out_path.display().to_string()];
    args.extend(sources);
    args.extend(spec.args.clone());
    util::run_command("gcc", &args, Some(repo_dir), Some(&spec.env))?;

    if !out_path.exists() {
        bail!("gcc did not produce {}", out_path.display());
    }
    Ok(out_path)
}

fn build_gpp(repo_dir: &Path, spec: &BuildSpec, name_hint: &str, out_dir: &Path) -> Result<PathBuf> {
    util::require_tool("g++")?;
    let sources = collect_sources(repo_dir, &["cpp", "cc", "cxx"])?;
    if sources.is_empty() {
        bail!("no C++ sources found for g++ build");
    }

    let bin_name = spec.bin.clone().unwrap_or_else(|| name_hint.to_string());
    let out_path = out_dir.join(&bin_name);

    let mut args = vec!["-O2".to_string(), "-o".to_string(), out_path.display().to_string()];
    args.extend(sources);
    args.extend(spec.args.clone());
    util::run_command("g++", &args, Some(repo_dir), Some(&spec.env))?;

    if !out_path.exists() {
        bail!("g++ did not produce {}", out_path.display());
    }
    Ok(out_path)
}

fn build_rustc(repo_dir: &Path, spec: &BuildSpec, name_hint: &str, out_dir: &Path) -> Result<PathBuf> {
    util::require_tool("rustc")?;

    let main_rs = if repo_dir.join("src").join("main.rs").exists() {
        repo_dir.join("src").join("main.rs")
    } else {
        repo_dir.join("main.rs")
    };

    if !main_rs.exists() {
        bail!("no main.rs found for rustc build");
    }

    let bin_name = spec.bin.clone().unwrap_or_else(|| name_hint.to_string());
    let out_path = out_dir.join(&bin_name);

    let mut args = vec![
        "-O".to_string(),
        main_rs.display().to_string(),
        "-o".to_string(),
        out_path.display().to_string(),
    ];
    args.extend(spec.args.clone());
    util::run_command("rustc", &args, Some(repo_dir), Some(&spec.env))?;

    if !out_path.exists() {
        bail!("rustc did not produce {}", out_path.display());
    }
    Ok(out_path)
}

#[derive(Debug, Clone)]
struct ExecutableInfo {
    path: PathBuf,
    size: u64,
    modified: Option<SystemTime>,
}

fn list_executables(root: &Path) -> Result<Vec<ExecutableInfo>> {
    let mut executables = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| entry.file_name() != std::ffi::OsStr::new(".git"))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let metadata = entry.metadata()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        let info = ExecutableInfo {
            path: entry.path().to_path_buf(),
            size: metadata.len(),
            modified: metadata.modified().ok(),
        };
        executables.push(info);
    }
    Ok(executables)
}

fn diff_executables(before: &[ExecutableInfo], after: &[ExecutableInfo]) -> Vec<ExecutableInfo> {
    let mut before_map = HashMap::new();
    for item in before {
        before_map.insert(item.path.clone(), item.clone());
    }

    let mut diff = Vec::new();
    for item in after {
        match before_map.get(&item.path) {
            None => diff.push(item.clone()),
            Some(previous) => {
                let modified = item.modified.and_then(|m| previous.modified.map(|p| m > p));
                if modified.unwrap_or(false) || item.size != previous.size {
                    diff.push(item.clone());
                }
            }
        }
    }

    diff
}

fn pick_executable(candidates: Vec<ExecutableInfo>, desired: Option<&str>) -> Result<PathBuf> {
    if candidates.is_empty() {
        bail!("no executable output found");
    }

    if let Some(name) = desired {
        for item in &candidates {
            if item.path.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Ok(item.path.clone());
            }
        }
    }

    if candidates.len() == 1 {
        return Ok(candidates[0].path.clone());
    }

    let mut sorted = candidates;
    sorted.sort_by_key(|item| item.size);
    Ok(sorted.last().unwrap().path.clone())
}

fn collect_sources(repo_dir: &Path, extensions: &[&str]) -> Result<Vec<String>> {
    let mut sources = Vec::new();
    for entry in fs::read_dir(repo_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if extensions.iter().any(|allowed| allowed.eq_ignore_ascii_case(ext)) {
                    sources.push(path.display().to_string());
                }
            }
        }
    }
    Ok(sources)
}
