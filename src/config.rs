use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Codex,
    Claude,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::Claude => "Claude",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub provider: Provider,
    pub variants: u8,
    pub instructions: Option<String>,
    pub codex_executable: PathBuf,
    pub claude_executable: PathBuf,
    pub model: Option<String>,
    pub claude_model: Option<String>,
    pub history_limit: usize,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ProjectConfig {
    provider: Option<Provider>,
    variants: Option<u8>,
    instructions: Option<String>,
    model: Option<String>,
    claude_model: Option<String>,
    history_limit: Option<usize>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: Provider::Codex,
            variants: 3,
            instructions: None,
            codex_executable: PathBuf::from("codex"),
            claude_executable: PathBuf::from("claude"),
            model: Some("gpt-5.4-mini".to_owned()),
            claude_model: Some("haiku".to_owned()),
            history_limit: 20,
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|directory| directory.join("gencommit/config.toml"))
    }

    pub fn load() -> Result<Self> {
        let Some(path) = Self::path() else {
            return Ok(Self::default());
        };
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        let config: Self =
            toml::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
        if !(1..=10).contains(&config.variants) {
            anyhow::bail!("config variants must be between 1 and 10");
        }
        Ok(config)
    }

    pub fn apply_project(&mut self, repository_root: &Path) -> Result<Option<PathBuf>> {
        let path = repository_root.join(".gencommit.toml");
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        let project: ProjectConfig =
            toml::from_str(&contents).with_context(|| format!("parse {}", path.display()))?;
        if let Some(variants) = project.variants {
            if !(1..=10).contains(&variants) {
                anyhow::bail!("{}: variants must be between 1 and 10", path.display());
            }
            self.variants = variants;
        }
        if let Some(history_limit) = project.history_limit {
            if history_limit == 0 {
                anyhow::bail!(
                    "{}: history_limit must be greater than zero",
                    path.display()
                );
            }
            self.history_limit = history_limit;
        }
        if let Some(provider) = project.provider {
            self.provider = provider;
        }
        if let Some(instructions) = project.instructions {
            self.instructions = Some(instructions);
        }
        if let Some(model) = project.model {
            self.model = Some(model);
        }
        if let Some(model) = project.claude_model {
            self.claude_model = Some(model);
        }
        Ok(Some(path))
    }

    pub fn init_project(&self, repository_root: &Path) -> Result<PathBuf> {
        let path = repository_root.join(".gencommit.toml");
        let mut contents = format!(
            "# Shared gencommit settings for this repository.\n\
             provider = \"{}\"\n\
             variants = {}\n\
             history_limit = {}\n",
            self.provider.key(),
            self.variants,
            self.history_limit
        );
        match self.provider {
            Provider::Codex => {
                if let Some(model) = &self.model {
                    contents.push_str(&format!("model = {}\n", toml_string(model)?));
                }
            }
            Provider::Claude => {
                if let Some(model) = &self.claude_model {
                    contents.push_str(&format!("claude_model = {}\n", toml_string(model)?));
                }
            }
        }
        if let Some(instructions) = &self.instructions {
            contents.push_str(&format!("instructions = {}\n", toml_string(instructions)?));
        } else {
            contents
                .push_str("# instructions = \"Use Conventional Commits with a package scope.\"\n");
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::AlreadyExists {
                    anyhow::anyhow!(
                        "{} already exists; refusing to overwrite it",
                        path.display()
                    )
                } else {
                    error.into()
                }
            })?;
        file.write_all(contents.as_bytes())
            .with_context(|| format!("write {}", path.display()))?;
        Ok(path)
    }

    pub fn set_model(&mut self, model: String) -> Result<PathBuf> {
        match self.provider {
            Provider::Codex => self.model = Some(model),
            Provider::Claude => self.claude_model = Some(model),
        }
        self.save()
    }

    pub fn set_provider(&mut self, provider: Provider) -> Result<PathBuf> {
        self.provider = provider;
        self.save()
    }

    fn save(&self) -> Result<PathBuf> {
        let path = Self::path().context("could not determine the user configuration directory")?;
        let parent = path.parent().context("invalid configuration path")?;
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        let mut file = NamedTempFile::new_in(parent)
            .with_context(|| format!("create temporary file in {}", parent.display()))?;
        std::io::Write::write_all(&mut file, toml::to_string_pretty(self)?.as_bytes())?;
        file.persist(&path)
            .with_context(|| format!("write {}", path.display()))?;
        Ok(path)
    }
}

fn toml_string(value: &str) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_config_overrides_generation_settings_only() {
        let repository = tempfile::tempdir().unwrap();
        fs::write(
            repository.path().join(".gencommit.toml"),
            r#"
                provider = "claude"
                variants = 5
                instructions = "Use Conventional Commits."
                claude_model = "sonnet"
                history_limit = 12
            "#,
        )
        .unwrap();
        let mut config = Config {
            codex_executable: PathBuf::from("custom-codex"),
            ..Config::default()
        };

        config.apply_project(repository.path()).unwrap();

        assert_eq!(config.provider, Provider::Claude);
        assert_eq!(config.variants, 5);
        assert_eq!(
            config.instructions.as_deref(),
            Some("Use Conventional Commits.")
        );
        assert_eq!(config.claude_model.as_deref(), Some("sonnet"));
        assert_eq!(config.history_limit, 12);
        assert_eq!(config.codex_executable, PathBuf::from("custom-codex"));
    }

    #[test]
    fn project_config_rejects_executable_overrides() {
        let repository = tempfile::tempdir().unwrap();
        fs::write(
            repository.path().join(".gencommit.toml"),
            "codex_executable = \"malicious-command\"\n",
        )
        .unwrap();

        let error = Config::default()
            .apply_project(repository.path())
            .unwrap_err();

        assert!(format!("{error:#}").contains("unknown field"));
    }

    #[test]
    fn initializes_project_from_user_generation_settings_without_executables() {
        let repository = tempfile::tempdir().unwrap();
        let config = Config {
            provider: Provider::Claude,
            claude_model: Some("sonnet".into()),
            instructions: Some("Keep it short.".into()),
            claude_executable: PathBuf::from("custom-claude"),
            ..Config::default()
        };

        let path = config.init_project(repository.path()).unwrap();
        let contents = fs::read_to_string(path).unwrap();

        assert!(contents.contains("provider = \"claude\""));
        assert!(contents.contains("claude_model = \"sonnet\""));
        assert!(contents.contains("instructions = \"Keep it short.\""));
        assert!(!contents.contains("custom-claude"));
        assert!(config.init_project(repository.path()).is_err());
    }
}
