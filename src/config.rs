use std::fs;
use std::path::PathBuf;

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
