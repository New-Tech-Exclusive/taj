use anyhow::{bail, Result};
use clap::{ArgGroup, Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "taj", version, about = "Taj package manager")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Install(InstallArgs),
    Uninstall { name: String },
    List,
}

#[derive(Args, Debug)]
#[command(group(ArgGroup::new("source").args(["name", "github", "gitlab"]).required(true)))]
pub struct InstallArgs {
    pub name: Option<String>,

    #[arg(long, value_name = "URL")]
    pub github: Option<String>,

    #[arg(long, value_name = "URL")]
    pub gitlab: Option<String>,
}

#[derive(Debug)]
pub enum LegacyCommand {
    InstallMirror { name: String },
    InstallGit { url: String, provider: String },
    Uninstall { name: String },
}

pub fn parse_legacy(args: &[String]) -> Result<Option<LegacyCommand>> {
    if args.len() < 2 {
        return Ok(None);
    }

    match args[1].as_str() {
        "-i" => {
            if args.len() < 3 {
                bail!("missing package name after -i");
            }
            Ok(Some(LegacyCommand::InstallMirror {
                name: args[2].clone(),
            }))
        }
        "-gh" => {
            if args.len() < 3 {
                bail!("missing URL after -gh");
            }
            Ok(Some(LegacyCommand::InstallGit {
                url: args[2].clone(),
                provider: "github".to_string(),
            }))
        }
        "-gl" => {
            if args.len() < 3 {
                bail!("missing URL after -gl");
            }
            Ok(Some(LegacyCommand::InstallGit {
                url: args[2].clone(),
                provider: "gitlab".to_string(),
            }))
        }
        "-u" => {
            if args.len() < 3 {
                bail!("missing package name after -u");
            }
            Ok(Some(LegacyCommand::Uninstall {
                name: args[2].clone(),
            }))
        }
        _ => Ok(None),
    }
}
