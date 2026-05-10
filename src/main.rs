mod build;
mod cli;
mod config;
mod detect;
mod git;
mod install;
mod state;
mod tajfile;
mod util;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use std::path::Path;

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {:#}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if let Some(legacy) = cli::parse_legacy(&args)? {
        return run_legacy(legacy);
    }

    let cli = cli::Cli::parse();
    if cli.command.is_none() {
        cli::Cli::command().print_help()?;
        println!();
        return Ok(());
    }

    let (config, _path) = config::Config::load_or_create()?;
    let state_path = config::state_file(&config)?;
    let mut state = state::State::load(Path::new(&state_path))?;

    match cli.command.unwrap() {
        cli::Command::Install(args) => {
            let record = if let Some(name) = args.name {
                install::install_from_mirror(&config, &mut state, &name)?
            } else if let Some(url) = args.github {
                install::install_from_git(&config, &mut state, &url, "github")?
            } else if let Some(url) = args.gitlab {
                install::install_from_git(&config, &mut state, &url, "gitlab")?
            } else {
                unreachable!("clap guarantees one source");
            };
            state.save(Path::new(&state_path))?;
            println!("installed {} -> {}", record.name, record.bin_path);
        }
        cli::Command::Uninstall { name } => {
            install::uninstall(&config, &mut state, &name)?;
            state.save(Path::new(&state_path))?;
            println!("uninstalled {}", name);
        }
        cli::Command::List => {
            if state.packages.is_empty() {
                println!("no packages installed");
            } else {
                let mut keys: Vec<_> = state.packages.keys().cloned().collect();
                keys.sort();
                for key in keys {
                    if let Some(record) = state.packages.get(&key) {
                        println!("{} -> {}", record.name, record.bin_path);
                    }
                }
            }
        }
    }

    Ok(())
}

fn run_legacy(cmd: cli::LegacyCommand) -> Result<()> {
    let (config, _path) = config::Config::load_or_create()?;
    let state_path = config::state_file(&config)?;
    let mut state = state::State::load(Path::new(&state_path))?;

    match cmd {
        cli::LegacyCommand::InstallMirror { name } => {
            let record = install::install_from_mirror(&config, &mut state, &name)?;
            state.save(Path::new(&state_path))?;
            println!("installed {} -> {}", record.name, record.bin_path);
        }
        cli::LegacyCommand::InstallGit { url, provider } => {
            let record = install::install_from_git(&config, &mut state, &url, &provider)?;
            state.save(Path::new(&state_path))?;
            println!("installed {} -> {}", record.name, record.bin_path);
        }
        cli::LegacyCommand::Uninstall { name } => {
            install::uninstall(&config, &mut state, &name)?;
            state.save(Path::new(&state_path))?;
            println!("uninstalled {}", name);
        }
    }

    Ok(())
}
