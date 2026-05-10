use crate::build;
use crate::config::{self, Config};
use crate::detect;
use crate::git;
use crate::state::{InstallRecord, State};
use crate::tajfile::TajFile;
use crate::util;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub fn install_from_mirror(config: &Config, state: &mut State, name: &str) -> Result<InstallRecord> {
    let taj = if mirror_mode(config) == MirrorMode::Http {
        fetch_taj_from_http(config, name)?
    } else {
        let cache_dir = config::cache_dir(config)?;
        util::ensure_dir(&cache_dir)?;

        let mirror_dir = git::ensure_mirror(config, &cache_dir)?;
        let taj_path = find_taj_file(&mirror_dir, &config.mirror_packages_dir, name)?;
        TajFile::load(&taj_path)?
    };

    let package_name = taj.name.clone().unwrap_or_else(|| name.to_string());
    if state.packages.contains_key(&package_name) {
        bail!("package '{}' is already installed", package_name);
    }

    let spec = taj.to_build_spec()?;
    install_from_repo(
        config,
        state,
        &package_name,
        &taj.repo,
        &spec,
        "mirror",
    )
}

pub fn install_from_git(
    config: &Config,
    state: &mut State,
    url: &str,
    provider: &str,
) -> Result<InstallRecord> {
    let name = util::repo_name_from_url(url);
    if state.packages.contains_key(&name) {
        bail!("package '{}' is already installed", name);
    }

    let cache_dir = config::cache_dir(config)?;
    util::ensure_dir(&cache_dir)?;

    let temp_dir = create_temp_build_dir(&cache_dir, &name)?;
    git::clone_repo(url, temp_dir.path())?;

    let spec = detect::detect_build_spec(temp_dir.path(), &name)?;
    install_from_cloned_repo(
        config,
        state,
        &name,
        url,
        &spec,
        provider,
        temp_dir.path(),
        temp_dir.path(),
    )
}

pub fn uninstall(config: &Config, state: &mut State, name: &str) -> Result<()> {
    let record = state
        .packages
        .remove(name)
        .ok_or_else(|| anyhow::anyhow!("package '{}' is not installed", name))?;

    let bin_path = Path::new(&record.bin_path);
    if bin_path.exists() {
        fs::remove_file(bin_path)
            .with_context(|| format!("failed to remove {}", bin_path.display()))?;
    }

    let install_dir = config::install_dir(config)?;
    if !install_dir.exists() {
        return Ok(());
    }

    Ok(())
}

fn install_from_repo(
    config: &Config,
    state: &mut State,
    name: &str,
    repo_url: &str,
    spec: &build::BuildSpec,
    source: &str,
) -> Result<InstallRecord> {
    let cache_dir = config::cache_dir(config)?;
    let temp_dir = create_temp_build_dir(&cache_dir, name)?;

    git::clone_repo(repo_url, temp_dir.path())?;
    let repo_dir = if let Some(subdir) = &spec.subdir {
        temp_dir.path().join(subdir)
    } else {
        temp_dir.path().to_path_buf()
    };

    install_from_cloned_repo(
        config,
        state,
        name,
        repo_url,
        spec,
        source,
        temp_dir.path(),
        &repo_dir,
    )
}

fn install_from_cloned_repo(
    config: &Config,
    state: &mut State,
    name: &str,
    repo_url: &str,
    spec: &build::BuildSpec,
    source: &str,
    build_root: &Path,
    repo_dir: &Path,
) -> Result<InstallRecord> {
    let install_dir = config::install_dir(config)?;
    validate_install_dir(&install_dir)?;

    let out_dir = build_root.join("out");
    util::ensure_dir(&out_dir)?;

    let built_path = build::build(repo_dir, spec, name, &out_dir)?;

    let install_name = spec.bin.clone().unwrap_or_else(|| name.to_string());
    let final_path = install_binary(&built_path, &install_dir, &install_name)?;

    let record = InstallRecord {
        name: name.to_string(),
        bin_name: install_name,
        bin_path: final_path.display().to_string(),
        repo: repo_url.to_string(),
        source: source.to_string(),
        installed_at: util::now_ts(),
    };

    state.packages.insert(name.to_string(), record.clone());
    Ok(record)
}

fn install_binary(source: &Path, install_dir: &Path, name: &str) -> Result<PathBuf> {
    util::ensure_dir(install_dir)?;

    let dest = install_dir.join(name);
    if dest.exists() {
        bail!("destination already exists: {}", dest.display());
    }

    fs::copy(source, &dest)
        .with_context(|| format!("failed to copy {} to {}", source.display(), dest.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&dest)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&dest, perms)?;
    }

    Ok(dest)
}

fn validate_install_dir(install_dir: &Path) -> Result<()> {
    if !install_dir.exists() {
        util::ensure_dir(install_dir)?;
    }

    if is_system_dir(install_dir) && !util::is_root() {
        bail!(
            "install dir {} requires root; run with sudo or set install_dir in config",
            install_dir.display()
        );
    }

    if !util::path_in_env(install_dir) {
        eprintln!(
            "warning: install dir {} is not in PATH",
            install_dir.display()
        );
    }

    Ok(())
}

fn is_system_dir(path: &Path) -> bool {
    let path_str = path.display().to_string();
    path_str.starts_with("/usr/")
        || path_str == "/usr"
        || path_str.starts_with("/bin")
        || path_str.starts_with("/sbin")
}

fn find_taj_file(mirror_dir: &Path, packages_dir: &str, name: &str) -> Result<PathBuf> {
    let mut candidates = Vec::new();

    let package_root = mirror_dir.join(packages_dir);
    candidates.push(package_root.join(format!("{}.taj", name)));
    candidates.push(package_root.join(format!("{}.Taj", name)));
    candidates.push(mirror_dir.join(format!("{}.taj", name)));
    candidates.push(mirror_dir.join(format!("{}.Taj", name)));

    for candidate in candidates {
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    bail!("no taj file found for {} in mirror", name)
}

fn create_temp_build_dir(cache_dir: &Path, name: &str) -> Result<tempfile::TempDir> {
    let tmp_root = cache_dir.join("tmp");
    util::ensure_dir(&tmp_root)?;
    tempfile::Builder::new()
        .prefix(&format!("taj-{}-", name))
        .tempdir_in(tmp_root)
        .context("failed to create temp build dir")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MirrorMode {
    Git,
    Http,
}

fn mirror_mode(config: &Config) -> MirrorMode {
    match config.mirror_mode.as_deref() {
        Some(mode) if mode.eq_ignore_ascii_case("http") => MirrorMode::Http,
        Some(mode) if mode.eq_ignore_ascii_case("https") => MirrorMode::Http,
        Some(mode) if mode.eq_ignore_ascii_case("url") => MirrorMode::Http,
        _ => MirrorMode::Git,
    }
}

fn fetch_taj_from_http(config: &Config, name: &str) -> Result<TajFile> {
    let bases = http_mirror_bases(config);
    for base in &bases {
        for ext in ["taj", "Taj"] {
            let url = format!("{}/{}.{}", base, name, ext);
            if let Some(content) = try_http_get_text(&url)? {
                let taj: TajFile = toml::from_str(&content).with_context(|| {
                    format!("failed to parse taj file from {}", url)
                })?;
                return Ok(taj);
            }
        }
    }

    bail!(
        "no taj file found for '{}' in HTTP mirror (bases: {})",
        name,
        bases.join(", ")
    )
}

fn http_mirror_bases(config: &Config) -> Vec<String> {
    let mut bases: Vec<String> = Vec::new();

    if let Some(raw) = github_raw_base(
        &config.mirror_repo,
        &config.mirror_branch,
        &config.mirror_packages_dir,
    ) {
        push_unique(&mut bases, raw);
    }

    let mut base = config.mirror_repo.trim_end_matches('/').to_string();
    let pkg_dir = config.mirror_packages_dir.trim_matches('/');
    if !pkg_dir.is_empty() && !base.ends_with(&format!("/{}", pkg_dir)) {
        base = format!("{}/{}", base, pkg_dir);
    }
    push_unique(&mut bases, base);

    bases
}

fn github_raw_base(repo_url: &str, branch: &str, packages_dir: &str) -> Option<String> {
    let trimmed = repo_url.trim().trim_end_matches('/').trim_end_matches(".git");
    let rest = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))?;

    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let repo = parts.next()?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    let mut base = format!(
        "https://raw.githubusercontent.com/{}/{}/{}",
        owner,
        repo.trim_end_matches(".git"),
        branch
    );
    let pkg_dir = packages_dir.trim_matches('/');
    if !pkg_dir.is_empty() {
        base.push('/');
        base.push_str(pkg_dir);
    }
    Some(base)
}

fn try_http_get_text(url: &str) -> Result<Option<String>> {
    match ureq::get(url).call() {
        Ok(resp) => {
            if resp.status() == 200 {
                let content = resp
                    .into_string()
                    .with_context(|| format!("failed to read body from {}", url))?;
                Ok(Some(content))
            } else if resp.status() == 404 {
                Ok(None)
            } else {
                bail!("mirror returned HTTP {} for {}", resp.status(), url)
            }
        }
        Err(ureq::Error::Status(code, resp)) => {
            if code == 404 {
                Ok(None)
            } else {
                let body = resp.into_string().unwrap_or_default();
                bail!("mirror returned HTTP {} for {}: {}", code, url, body)
            }
        }
        Err(ureq::Error::Transport(err)) => {
            bail!("failed to fetch {}: {}", url, err)
        }
    }
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.iter().any(|existing| existing == &value) {
        list.push(value);
    }
}

