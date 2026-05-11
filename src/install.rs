use crate::build;
use crate::config::{self, Config};
use crate::detect;
use crate::git;
use crate::state::{InstallRecord, State};
use crate::tajfile::{InstallSpec, TajFile};
use crate::util;
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMode {
    Install,
    Upgrade,
}

pub fn install_from_mirror(config: &Config, state: &mut State, name: &str) -> Result<InstallRecord> {
    install_from_mirror_inner(config, state, name, InstallMode::Install)
}

pub fn upgrade_from_mirror(config: &Config, state: &mut State, name: &str) -> Result<InstallRecord> {
    install_from_mirror_inner(config, state, name, InstallMode::Upgrade)
}

fn install_from_mirror_inner(
    config: &Config,
    state: &mut State,
    name: &str,
    mode: InstallMode,
) -> Result<InstallRecord> {
    let mut visiting = HashSet::new();
    let mut installed = HashSet::new();
    install_mirror_recursive(config, state, name, mode, true, &mut visiting, &mut installed)
}

pub fn install_from_git(
    config: &Config,
    state: &mut State,
    url: &str,
    provider: &str,
) -> Result<InstallRecord> {
    install_from_git_inner(config, state, url, provider, InstallMode::Install)
}

pub fn upgrade_from_git(
    config: &Config,
    state: &mut State,
    url: &str,
    provider: &str,
) -> Result<InstallRecord> {
    install_from_git_inner(config, state, url, provider, InstallMode::Upgrade)
}

fn install_from_git_inner(
    config: &Config,
    state: &mut State,
    url: &str,
    provider: &str,
    mode: InstallMode,
) -> Result<InstallRecord> {
    let name = util::repo_name_from_url(url);
    if state.packages.contains_key(&name) && mode == InstallMode::Install {
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
        None,
        None,
        provider,
        mode,
        temp_dir.path(),
        temp_dir.path(),
    )
}

pub fn upgrade_package(
    config: &Config,
    state: &mut State,
    name: &str,
) -> Result<InstallRecord> {
    let record = state
        .packages
        .get(name)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("package '{}' is not installed", name))?;

    match record.source.as_str() {
        "mirror" => {
            let source_name = record.source_name.as_deref().unwrap_or(name);
            upgrade_from_mirror(config, state, source_name)
        }
        "github" | "gitlab" => upgrade_from_git(config, state, &record.repo, &record.source),
        other => bail!("unknown source '{}' for package '{}'", other, name),
    }
}

pub fn upgrade_all(config: &Config, state: &mut State) -> Result<Vec<InstallRecord>> {
    let mut names: Vec<String> = state.packages.keys().cloned().collect();
    names.sort();

    let mut upgraded = Vec::new();
    for name in names {
        upgraded.push(upgrade_package(config, state, &name)?);
    }
    Ok(upgraded)
}

pub fn uninstall(config: &Config, state: &mut State, name: &str) -> Result<()> {
    let record = state
        .packages
        .remove(name)
        .ok_or_else(|| anyhow::anyhow!("package '{}' is not installed", name))?;

    let mut paths = record_file_list(&record);
    paths.sort();
    paths.dedup();

    let install_root = record
        .install_root
        .as_deref()
        .map(Path::new)
        .map(Path::to_path_buf);

    for path in paths {
        if let Some(owner) = find_owner(state, &path, None) {
            eprintln!("warning: skipping {} (owned by {})", path, owner);
            continue;
        }
        let target = Path::new(&path);
        if !target.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(target)
            .with_context(|| format!("failed to read metadata for {}", target.display()))?;
        if metadata.is_dir() {
            fs::remove_dir_all(target)
                .with_context(|| format!("failed to remove {}", target.display()))?;
        } else {
            fs::remove_file(target)
                .with_context(|| format!("failed to remove {}", target.display()))?;
        }

        if let Some(root) = &install_root {
            cleanup_empty_dirs(root, target)?;
        }
    }

    let install_dir = config::install_dir(config)?;
    if !install_dir.exists() {
        return Ok(());
    }

    Ok(())
}

fn install_mirror_recursive(
    config: &Config,
    state: &mut State,
    name: &str,
    mode: InstallMode,
    strict_existing: bool,
    visiting: &mut HashSet<String>,
    installed: &mut HashSet<String>,
) -> Result<InstallRecord> {
    if visiting.contains(name) {
        bail!("dependency cycle detected at '{}'", name);
    }
    visiting.insert(name.to_string());

    let (taj, source_name) = load_taj_from_mirror(config, name)?;
    let package_name = taj.name.clone().unwrap_or_else(|| name.to_string());

    if installed.contains(&package_name) {
        visiting.remove(name);
        return state
            .packages
            .get(&package_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing state for {}", package_name));
    }

    if mode == InstallMode::Install && strict_existing && state.packages.contains_key(&package_name) {
        visiting.remove(name);
        bail!("package '{}' is already installed", package_name);
    }

    if mode == InstallMode::Install && !strict_existing && state.packages.contains_key(&package_name) {
        visiting.remove(name);
        return state
            .packages
            .get(&package_name)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("missing state for {}", package_name));
    }

    if let Some(deps) = &taj.depends {
        for dep in deps {
            install_mirror_recursive(
                config,
                state,
                dep,
                InstallMode::Install,
                false,
                visiting,
                installed,
            )?;
        }
    }

    if is_meta_build(&taj) {
        preflight_deps(&taj)?;
        let record = install_meta_package(
            state,
            &package_name,
            &taj.repo,
            Some(source_name),
            "mirror",
        )?;
        installed.insert(package_name);
        visiting.remove(name);
        return Ok(record);
    }

    preflight_deps(&taj)?;
    let spec = taj.to_build_spec()?;
    let record = install_from_repo(
        config,
        state,
        &package_name,
        &taj.repo,
        taj.git_ref.as_deref(),
        &spec,
        taj.install.as_ref(),
        Some(source_name),
        "mirror",
        mode,
    )?;

    installed.insert(package_name);
    visiting.remove(name);
    Ok(record)
}

fn load_taj_from_mirror(config: &Config, name: &str) -> Result<(TajFile, String)> {
    let taj = if mirror_mode(config) == MirrorMode::Http {
        fetch_taj_from_http(config, name)?
    } else {
        let cache_dir = config::cache_dir(config)?;
        util::ensure_dir(&cache_dir)?;

        let mirror_dir = git::ensure_mirror(config, &cache_dir)?;
        let taj_path = find_taj_file(&mirror_dir, &config.mirror_packages_dir, name)?;
        TajFile::load(&taj_path)?
    };

    Ok((taj, name.to_string()))
}

fn install_from_repo(
    config: &Config,
    state: &mut State,
    name: &str,
    repo_url: &str,
    repo_ref: Option<&str>,
    spec: &build::BuildSpec,
    install: Option<&InstallSpec>,
    source_name: Option<String>,
    source: &str,
    mode: InstallMode,
) -> Result<InstallRecord> {
    let cache_dir = config::cache_dir(config)?;
    let temp_dir = create_temp_build_dir(&cache_dir, name)?;

    git::clone_repo_at_ref(repo_url, temp_dir.path(), repo_ref)?;
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
        install,
        source_name,
        source,
        mode,
        temp_dir.path(),
        &repo_dir,
    )
}

fn is_meta_build(taj: &TajFile) -> bool {
    taj.build.eq_ignore_ascii_case("meta")
}

fn install_meta_package(
    state: &mut State,
    name: &str,
    repo_url: &str,
    source_name: Option<String>,
    source: &str,
) -> Result<InstallRecord> {
    let record = InstallRecord {
        name: name.to_string(),
        bin_name: name.to_string(),
        bin_path: String::new(),
        repo: repo_url.to_string(),
        source: source.to_string(),
        installed_at: util::now_ts(),
        files: Vec::new(),
        install_root: None,
        source_name,
        is_meta: true,
    };

    state.packages.insert(name.to_string(), record.clone());
    Ok(record)
}

fn install_from_cloned_repo(
    config: &Config,
    state: &mut State,
    name: &str,
    repo_url: &str,
    spec: &build::BuildSpec,
    install: Option<&InstallSpec>,
    source_name: Option<String>,
    source: &str,
    mode: InstallMode,
    build_root: &Path,
    repo_dir: &Path,
) -> Result<InstallRecord> {
    let install_dir = config::install_dir(config)?;
    validate_install_dir(&install_dir)?;

    let out_dir = build_root.join("out");
    util::ensure_dir(&out_dir)?;

    let overwrite = mode == InstallMode::Upgrade;
    let previous = if overwrite {
        state.packages.get(name).cloned()
    } else {
        None
    };

    let (bin_name, bin_path, files, install_root) = if let Some(install_spec) = install {
        let prefix = resolve_install_prefix(&install_dir, install_spec);
        validate_install_prefix(&prefix)?;
        let build_spec = build_spec_with_prefix(spec, &prefix);

        build::build_no_output(repo_dir, &build_spec, name, &out_dir)?;
        let stage_root = run_install(install_spec, repo_dir, build_root, &prefix, &build_spec)?;
        let staged_targets = collect_stage_destinations(&stage_root, &prefix)?;
        check_install_conflicts(state, name, &staged_targets, overwrite)?;
        let files = install_from_stage(&stage_root, &prefix, overwrite)?;
        let bin_name = build_spec.bin.clone().unwrap_or_else(|| name.to_string());
        let bin_path = primary_bin_path(&prefix, build_spec.bin.as_deref(), name)
            .unwrap_or_else(|| prefix.display().to_string());
        (bin_name, bin_path, files, Some(prefix.display().to_string()))
    } else {
        let built_path = build::build(repo_dir, spec, name, &out_dir)?;

        let install_name = spec.bin.clone().unwrap_or_else(|| name.to_string());
        let dest = install_dir.join(&install_name);
        check_install_conflicts(state, name, &[dest], overwrite)?;
        let final_path = install_binary(&built_path, &install_dir, &install_name, overwrite)?;
        (
            install_name,
            final_path.display().to_string(),
            vec![final_path.display().to_string()],
            None,
        )
    };

    let record = InstallRecord {
        name: name.to_string(),
        bin_name,
        bin_path,
        repo: repo_url.to_string(),
        source: source.to_string(),
        installed_at: util::now_ts(),
        files,
        install_root,
        source_name,
        is_meta: false,
    };

    if let Some(previous) = previous {
        if let Err(err) = cleanup_stale_files(state, &previous, &record) {
            eprintln!("warning: failed to remove stale files for {}: {}", name, err);
        }
    }

    state.packages.insert(name.to_string(), record.clone());
    Ok(record)
}

fn install_binary(source: &Path, install_dir: &Path, name: &str, overwrite: bool) -> Result<PathBuf> {
    util::ensure_dir(install_dir)?;

    let dest = install_dir.join(name);
    if dest.exists() {
        if overwrite {
            fs::remove_file(&dest)
                .with_context(|| format!("failed to remove {}", dest.display()))?;
        } else {
            bail!("destination already exists: {}", dest.display());
        }
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

fn install_prefix(install_dir: &Path) -> PathBuf {
    install_dir
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| install_dir.to_path_buf())
}

fn resolve_install_prefix(install_dir: &Path, install_spec: &InstallSpec) -> PathBuf {
    if let Some(prefix) = &install_spec.prefix {
        PathBuf::from(prefix)
    } else {
        install_prefix(install_dir)
    }
}

fn build_spec_with_prefix(spec: &build::BuildSpec, prefix: &Path) -> build::BuildSpec {
    let mut adjusted = spec.clone();
    let prefix_value = prefix.display().to_string();

    match adjusted.method {
        build::BuildMethod::Autotools => {
            if !has_flag_with_value(&adjusted.args, "--prefix") {
                adjusted.args.push(format!("--prefix={}", prefix_value));
            }
        }
        build::BuildMethod::Cmake => {
            if !has_cmake_prefix_arg(&adjusted.args) {
                adjusted
                    .args
                    .push(format!("-DCMAKE_INSTALL_PREFIX={}", prefix_value));
            }
        }
        build::BuildMethod::Meson => {
            if !has_flag_with_value(&adjusted.args, "--prefix") {
                adjusted.args.push(format!("--prefix={}", prefix_value));
            }
        }
        _ => {}
    }

    adjusted
}

fn has_flag_with_value(args: &[String], flag: &str) -> bool {
    let prefix = format!("{}=", flag);
    args.iter().any(|arg| arg == flag || arg.starts_with(&prefix))
}

fn has_cmake_prefix_arg(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "-DCMAKE_INSTALL_PREFIX" || arg.starts_with("-DCMAKE_INSTALL_PREFIX=")
    })
}

fn validate_install_prefix(prefix: &Path) -> Result<()> {
    if !prefix.exists() {
        util::ensure_dir(prefix)?;
    }

    if is_system_dir(prefix) && !util::is_root() {
        bail!(
            "install prefix {} requires root; run with sudo or set install_dir in config",
            prefix.display()
        );
    }

    let bin_dir = prefix.join("bin");
    if bin_dir.exists() && !util::path_in_env(&bin_dir) {
        eprintln!("warning: {} is not in PATH", bin_dir.display());
    }

    Ok(())
}

fn run_install(
    spec: &InstallSpec,
    repo_dir: &Path,
    build_root: &Path,
    prefix: &Path,
    build_spec: &build::BuildSpec,
) -> Result<PathBuf> {
    let stage_dir = build_root.join("stage");
    util::ensure_dir(&stage_dir)?;

    let destdir = spec
        .destdir
        .clone()
        .unwrap_or_else(|| stage_dir.display().to_string());
    let prefix_value = spec
        .prefix
        .clone()
        .unwrap_or_else(|| prefix.display().to_string());

    let work_dir = install_work_dir(repo_dir, build_spec)?;

    let method = spec.method.to_lowercase();
    match method.as_str() {
        "make" => run_make_install(spec, &work_dir, &destdir, &prefix_value, &build_spec.env),
        "cmake" => run_cmake_install(
            spec,
            repo_dir,
            &work_dir,
            &destdir,
            &prefix_value,
            &build_spec.env,
        ),
        "meson" => run_meson_install(
            spec,
            repo_dir,
            &work_dir,
            &destdir,
            &prefix_value,
            &build_spec.env,
        ),
        _ => bail!("unsupported install method: {}", spec.method),
    }?;

    Ok(PathBuf::from(destdir))
}

fn install_work_dir(repo_dir: &Path, build_spec: &build::BuildSpec) -> Result<PathBuf> {
    if let Some(build_dir) = &build_spec.build_dir {
        let dir = repo_dir.join(build_dir);
        util::ensure_dir(&dir)?;
        return Ok(dir);
    }

    match build_spec.method {
        build::BuildMethod::Cmake | build::BuildMethod::Meson => {
            let dir = repo_dir.join("build");
            util::ensure_dir(&dir)?;
            Ok(dir)
        }
        _ => Ok(repo_dir.to_path_buf()),
    }
}

fn run_make_install(
    spec: &InstallSpec,
    work_dir: &Path,
    destdir: &str,
    prefix: &str,
    build_env: &std::collections::HashMap<String, String>,
) -> Result<()> {
    util::require_tool("make")?;

    let mut args: Vec<String> = Vec::new();
    if let Some(values) = &spec.args {
        args.extend(values.clone());
    }
    if !args.iter().any(|value| value == "install") {
        args.insert(0, "install".to_string());
    }
    if !args.iter().any(|value| value.starts_with("DESTDIR=")) {
        args.push(format!("DESTDIR={}", destdir));
    }
    if !args.iter().any(|value| value.starts_with("prefix=")) {
        args.push(format!("prefix={}", prefix));
    }
    if !args.iter().any(|value| value.starts_with("PREFIX=")) {
        args.push(format!("PREFIX={}", prefix));
    }

    let mut env = build_env.clone();
    if let Some(values) = &spec.env {
        for (key, value) in values {
            env.insert(key.clone(), value.clone());
        }
    }
    env.entry("DESTDIR".to_string())
        .or_insert_with(|| destdir.to_string());
    env.entry("PREFIX".to_string())
        .or_insert_with(|| prefix.to_string());
    env.entry("prefix".to_string())
        .or_insert_with(|| prefix.to_string());

    util::run_command("make", &args, Some(work_dir), Some(&env))?;
    Ok(())
}

fn run_cmake_install(
    spec: &InstallSpec,
    repo_dir: &Path,
    build_dir: &Path,
    destdir: &str,
    prefix: &str,
    build_env: &std::collections::HashMap<String, String>,
) -> Result<()> {
    util::require_tool("cmake")?;

    let install_dir = if build_dir.exists() {
        build_dir.to_path_buf()
    } else {
        repo_dir.to_path_buf()
    };

    let mut args = vec![
        "--install".to_string(),
        install_dir.display().to_string(),
        "--prefix".to_string(),
        prefix.to_string(),
    ];
    if let Some(values) = &spec.args {
        args.extend(values.clone());
    }

    let mut env = build_env.clone();
    if let Some(values) = &spec.env {
        for (key, value) in values {
            env.insert(key.clone(), value.clone());
        }
    }
    env.entry("DESTDIR".to_string())
        .or_insert_with(|| destdir.to_string());

    util::run_command("cmake", &args, Some(repo_dir), Some(&env))?;
    Ok(())
}

fn run_meson_install(
    spec: &InstallSpec,
    repo_dir: &Path,
    build_dir: &Path,
    destdir: &str,
    prefix: &str,
    build_env: &std::collections::HashMap<String, String>,
) -> Result<()> {
    util::require_tool("meson")?;
    util::require_tool("ninja")?;
    util::ensure_dir(build_dir)?;

    let mut env = build_env.clone();
    if let Some(values) = &spec.env {
        for (key, value) in values {
            env.insert(key.clone(), value.clone());
        }
    }
    env.entry("DESTDIR".to_string())
        .or_insert_with(|| destdir.to_string());

    let configured = build_dir.join("build.ninja").exists();
    let mut setup_args = vec!["setup".to_string()];
    if configured {
        setup_args.push("--reconfigure".to_string());
    }
    setup_args.push(build_dir.display().to_string());
    setup_args.push(format!("--prefix={}", prefix));
    if let Some(values) = &spec.args {
        setup_args.extend(values.clone());
    }
    util::run_command("meson", &setup_args, Some(repo_dir), Some(&env))?;

    let compile_args = vec![
        "compile".to_string(),
        "-C".to_string(),
        build_dir.display().to_string(),
    ];
    util::run_command("meson", &compile_args, Some(repo_dir), Some(&env))?;

    let install_args = vec![
        "install".to_string(),
        "-C".to_string(),
        build_dir.display().to_string(),
        "--destdir".to_string(),
        destdir.to_string(),
    ];
    util::run_command("meson", &install_args, Some(repo_dir), Some(&env))?;
    Ok(())
}

fn collect_stage_destinations(stage_root: &Path, prefix: &Path) -> Result<Vec<PathBuf>> {
    let payload_root = stage_payload_root(stage_root, prefix);
    if !payload_root.exists() {
        bail!("install produced no files in {}", stage_root.display());
    }

    let mut dests: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for entry in WalkDir::new(&payload_root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_dir() {
            continue;
        }

        let rel = path.strip_prefix(&payload_root).with_context(|| {
            format!("failed to build relative path for {}", path.display())
        })?;
        let dest = prefix.join(rel);
        if !seen.insert(dest.clone()) {
            bail!("duplicate staged path: {}", dest.display());
        }
        dests.push(dest);
    }

    if dests.is_empty() {
        bail!("install produced no files in {}", stage_root.display());
    }

    Ok(dests)
}

fn install_from_stage(stage_root: &Path, prefix: &Path, overwrite: bool) -> Result<Vec<String>> {
    let payload_root = stage_payload_root(stage_root, prefix);
    if !payload_root.exists() {
        bail!("install produced no files in {}", stage_root.display());
    }

    let mut installed: Vec<String> = Vec::new();
    for entry in WalkDir::new(&payload_root).follow_links(false) {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_dir() {
            continue;
        }

        let rel = path.strip_prefix(&payload_root).with_context(|| {
            format!("failed to build relative path for {}", path.display())
        })?;
        let dest = prefix.join(rel);

        if dest.exists() {
            if overwrite {
                let dest_meta = fs::symlink_metadata(&dest)?;
                if dest_meta.is_dir() {
                    bail!("destination is a directory: {}", dest.display());
                }
                fs::remove_file(&dest)
                    .with_context(|| format!("failed to remove {}", dest.display()))?;
            } else {
                bail!("destination already exists: {}", dest.display());
            }
        }
        if let Some(parent) = dest.parent() {
            util::ensure_dir(parent)?;
        }

        let metadata = fs::symlink_metadata(path)?;
        let is_symlink = metadata.file_type().is_symlink();
        if is_symlink {
            #[cfg(unix)]
            {
                use std::os::unix::fs::symlink;
                let target = fs::read_link(path)?;
                symlink(&target, &dest)?;
            }
            #[cfg(not(unix))]
            {
                fs::copy(path, &dest).with_context(|| {
                    format!("failed to copy {} to {}", path.display(), dest.display())
                })?;
            }
        } else {
            fs::copy(path, &dest).with_context(|| {
                format!("failed to copy {} to {}", path.display(), dest.display())
            })?;
        }

        if !is_symlink {
            #[cfg(unix)]
            {
                fs::set_permissions(&dest, metadata.permissions())?;
            }
        }

        installed.push(dest.display().to_string());
    }

    if installed.is_empty() {
        bail!("install produced no files in {}", stage_root.display());
    }

    Ok(installed)
}

fn stage_payload_root(stage_root: &Path, prefix: &Path) -> PathBuf {
    if prefix.is_absolute() {
        if let Ok(relative) = prefix.strip_prefix("/") {
            let staged = stage_root.join(relative);
            if staged.exists() {
                return staged;
            }
        }
    }
    stage_root.to_path_buf()
}

fn primary_bin_path(prefix: &Path, bin_name: Option<&str>, name: &str) -> Option<String> {
    let candidate = prefix.join("bin").join(bin_name.unwrap_or(name));
    if candidate.exists() {
        Some(candidate.display().to_string())
    } else {
        None
    }
}

fn record_file_list(record: &InstallRecord) -> Vec<String> {
    if record.is_meta {
        Vec::new()
    } else if record.files.is_empty() {
        vec![record.bin_path.clone()]
    } else {
        record.files.clone()
    }
}

fn build_ownership_index(state: &State) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for (name, record) in &state.packages {
        for path in record_file_list(record) {
            index.entry(path).or_insert_with(|| name.clone());
        }
    }
    index
}

fn find_owner(state: &State, path: &str, exclude: Option<&str>) -> Option<String> {
    for (name, record) in &state.packages {
        if exclude == Some(name.as_str()) {
            continue;
        }
        for record_path in record_file_list(record) {
            if record_path == path {
                return Some(name.clone());
            }
        }
    }
    None
}

fn check_install_conflicts(
    state: &State,
    owner: &str,
    targets: &[PathBuf],
    overwrite: bool,
) -> Result<()> {
    let ownership = build_ownership_index(state);
    let mut conflicts: Vec<String> = Vec::new();

    for target in targets {
        let path_str = target.display().to_string();
        if let Some(existing_owner) = ownership.get(&path_str) {
            if existing_owner != owner {
                conflicts.push(format!("{} (owned by {})", path_str, existing_owner));
            }
            continue;
        }

        if target.exists() {
            if overwrite {
                conflicts.push(format!("{} (exists on disk)", path_str));
            } else {
                conflicts.push(format!("{} (exists on disk)", path_str));
            }
        }
    }

    if !conflicts.is_empty() {
        bail!("file conflict(s) detected:\n- {}", conflicts.join("\n- "));
    }

    Ok(())
}

fn cleanup_stale_files(state: &State, previous: &InstallRecord, current: &InstallRecord) -> Result<()> {
    let mut stale = record_file_list(previous);
    stale.sort();
    stale.dedup();

    let current_set: HashSet<String> = record_file_list(current).into_iter().collect();
    let install_root = previous
        .install_root
        .as_deref()
        .map(Path::new)
        .map(Path::to_path_buf);

    for path in stale {
        if current_set.contains(&path) {
            continue;
        }

        if find_owner(state, &path, Some(&previous.name)).is_some() {
            continue;
        }

        let target = Path::new(&path);
        if !target.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(target)
            .with_context(|| format!("failed to read metadata for {}", target.display()))?;
        if metadata.is_dir() {
            fs::remove_dir_all(target)
                .with_context(|| format!("failed to remove {}", target.display()))?;
        } else {
            fs::remove_file(target)
                .with_context(|| format!("failed to remove {}", target.display()))?;
        }

        if let Some(root) = &install_root {
            cleanup_empty_dirs(root, target)?;
        }
    }

    Ok(())
}

fn cleanup_empty_dirs(root: &Path, leaf: &Path) -> Result<()> {
    let mut current = leaf.parent();
    while let Some(dir) = current {
        if !dir.starts_with(root) || dir == root {
            break;
        }
        let mut entries = fs::read_dir(dir)?;
        if entries.next().is_none() {
            fs::remove_dir(dir)?;
        } else {
            break;
        }
        current = dir.parent();
    }
    Ok(())
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

fn preflight_deps(taj: &TajFile) -> Result<()> {
    let Some(deps) = &taj.deps else {
        return Ok(());
    };

    let mut missing_tools: Vec<String> = Vec::new();
    if let Some(tools) = &deps.tools {
        for tool in tools {
            if util::require_tool(tool).is_err() {
                missing_tools.push(tool.to_string());
            }
        }
    }

    let mut missing_pkg: Vec<String> = Vec::new();
    if let Some(pkgs) = &deps.pkg_config {
        if util::require_tool("pkg-config").is_err() {
            missing_tools.push("pkg-config".to_string());
        } else {
            for pkg in pkgs {
                if !pkg_config_exists(pkg)? {
                    missing_pkg.push(pkg.to_string());
                }
            }
        }
    }

    if missing_tools.is_empty() && missing_pkg.is_empty() {
        return Ok(());
    }

    let mut message = String::from("missing build dependencies");
    if !missing_tools.is_empty() {
        message.push_str(": tools [");
        message.push_str(&missing_tools.join(", "));
        message.push(']');
    }
    if !missing_pkg.is_empty() {
        if !missing_tools.is_empty() {
            message.push_str("; ");
        } else {
            message.push_str(": ");
        }
        message.push_str("pkg-config [");
        message.push_str(&missing_pkg.join(", "));
        message.push(']');
    }

    if let Some(hints) = &deps.packages {
        let mut keys: Vec<_> = hints.keys().collect();
        keys.sort();
        if !keys.is_empty() {
            message.push_str("\ninstall hints:");
            for key in keys {
                if let Some(values) = hints.get(key) {
                    message.push_str(&format!("\n- {}: {}", key, values.join(" ")));
                }
            }
        }
    }

    if let Some(note) = &deps.message {
        message.push_str("\n");
        message.push_str(note);
    }

    bail!(message)
}

fn pkg_config_exists(name: &str) -> Result<bool> {
    let status = Command::new("pkg-config")
        .arg("--exists")
        .arg(name)
        .status()
        .with_context(|| format!("failed to run pkg-config for {}", name))?;
    Ok(status.success())
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

