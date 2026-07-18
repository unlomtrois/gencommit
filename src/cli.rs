use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "gencommit", version, about)]
pub struct Cli {
    /// Number of commit message variants to generate.
    #[arg(short = 'v', long, value_parser = clap::value_parser!(u8).range(1..=10))]
    pub variants: Option<u8>,

    /// Select every changed path. This is also the default when no paths are given.
    #[arg(long, conflicts_with = "paths")]
    pub all: bool,

    /// Git pathspecs to stage and commit.
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Manage the Codex/ChatGPT login used by gencommit.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
    /// Choose the Codex model used for commit generation.
    Model,
}

#[derive(Debug, Subcommand)]
pub enum AuthCommand {
    /// Open the Codex ChatGPT login flow.
    Login,
    /// Show the current Codex login status.
    Status,
}
