mod claude;
mod cli;
mod codex;
mod config;
mod git;
mod selector;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use anyhow::{Result, bail};
use clap::Parser;

use crate::claude::Claude;
use crate::cli::{AuthCommand, Cli, Command};
use crate::codex::Codex;
use crate::config::{Config, Provider};
use crate::git::{Repository, Selection};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let mut config = Config::load()?;
    let codex = Codex::new(config.codex_executable.clone(), config.model.clone());
    let claude = Claude::new(
        config.claude_executable.clone(),
        config.claude_model.clone(),
    );

    if let Some(command) = cli.command {
        return match command {
            Command::Initialize => {
                let repo = Repository::discover()?;
                let path = config.init_project(repo.root())?;
                println!("Created {}", path.display());
                Ok(())
            }
            Command::Auth { command } => match command {
                AuthCommand::Login => match config.provider {
                    Provider::Codex => codex.login(),
                    Provider::Claude => claude.login(),
                },
                AuthCommand::Status => match config.provider {
                    Provider::Codex => codex.login_status(),
                    Provider::Claude => claude.login_status(),
                },
            },
            Command::Model => {
                if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                    bail!("model selection requires a terminal");
                }
                let (models, current) = match config.provider {
                    Provider::Codex => (codex.models()?, config.model.as_deref()),
                    Provider::Claude => (claude.models(), config.claude_model.as_deref()),
                };
                if let Some(model) = selector::select_model(&models, current)? {
                    let slug = model.slug.clone();
                    let path = config.set_model(slug.clone())?;
                    println!("Selected {slug}; saved to {}", path.display());
                }
                Ok(())
            }
            Command::Provider => {
                if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                    bail!("provider selection requires a terminal");
                }
                if let Some(provider) = selector::select_provider(config.provider)? {
                    let path = config.set_provider(provider)?;
                    println!("Selected {}; saved to {}", provider.name(), path.display());
                }
                Ok(())
            }
        };
    }

    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("interactive generation requires a terminal");
    }

    let selection = if cli.all || cli.paths.is_empty() {
        Selection::All
    } else {
        Selection::Paths(cli.paths)
    };

    let repo = Repository::discover()?;
    let project_config = config.apply_project(repo.root())?;
    let variants = cli.variants.unwrap_or(config.variants);
    let codex = Codex::new(config.codex_executable.clone(), config.model.clone());
    let claude = Claude::new(
        config.claude_executable.clone(),
        config.claude_model.clone(),
    );
    repo.ensure_no_conflicts(&selection)?;
    let mut snapshot = repo.snapshot(&selection)?;

    println!("Repository: {}", repo.root().display());
    if let Some(path) = project_config {
        println!("Project config: {}", path.display());
    }
    println!(
        "Changes: {} file(s), {} patch bytes",
        snapshot.files,
        snapshot.patch.len()
    );
    println!(
        "The selected diff will be sent to {}.",
        config.provider.name()
    );

    loop {
        let history = repo.recent_subjects(config.history_limit)?;
        let messages = match config.provider {
            Provider::Codex => codex.generate(
                repo.root(),
                &snapshot.patch,
                &history,
                variants,
                config.instructions.as_deref(),
            )?,
            Provider::Claude => claude.generate(
                repo.root(),
                &snapshot.patch,
                &history,
                variants,
                config.instructions.as_deref(),
            )?,
        };

        match selector::select(&messages)? {
            selector::Action::Cancel => return Ok(()),
            selector::Action::Regenerate => {
                snapshot = repo.snapshot(&selection)?;
            }
            selector::Action::Select(message) => {
                println!("\nSelected commit message:\n\n{}\n", message.render());
                println!("Will stage and commit: {}", selection.describe());
                print!("Proceed? [y/N] ");
                io::stdout().flush()?;
                let mut answer = String::new();
                io::stdin().read_line(&mut answer)?;
                if !matches!(answer.trim(), "y" | "Y" | "yes" | "YES" | "Yes") {
                    println!("Cancelled; Git was not changed.");
                    return Ok(());
                }

                let current = repo.snapshot(&selection)?;
                if current.digest != snapshot.digest {
                    bail!("selected changes changed after generation; run gencommit again");
                }

                let commit = repo.stage_and_commit(&selection, &message.render())?;
                println!("Committed {} {}", commit.hash, commit.subject);
                return Ok(());
            }
        }
    }
}
