use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::tempdir;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CommitMessage {
    pub subject: String,
    #[serde(default)]
    pub body: Option<String>,
}

impl CommitMessage {
    pub fn render(&self) -> String {
        match self
            .body
            .as_deref()
            .map(str::trim)
            .filter(|body| !body.is_empty())
        {
            Some(body) => format!("{}\n\n{}", self.subject.trim(), body),
            None => self.subject.trim().to_owned(),
        }
    }

    pub(crate) fn validate(&self, provider: &str) -> Result<()> {
        if self.subject.trim().is_empty() || self.subject.contains('\n') {
            bail!("{provider} returned an empty or multiline commit subject");
        }
        if self.subject.chars().count() > 100 {
            bail!("{provider} returned a commit subject longer than 100 characters");
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct Generation {
    pub variants: Vec<CommitMessage>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Model {
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub(crate) visibility: String,
}

#[derive(Debug, Deserialize)]
struct ModelCatalog {
    models: Vec<Model>,
}

pub struct Codex {
    executable: PathBuf,
    model: Option<String>,
}

impl Codex {
    pub fn new(executable: PathBuf, model: Option<String>) -> Self {
        Self { executable, model }
    }

    pub fn login(&self) -> Result<()> {
        self.run_passthrough(["login"])
    }

    pub fn login_status(&self) -> Result<()> {
        self.run_passthrough(["login", "status"])
    }

    pub fn models(&self) -> Result<Vec<Model>> {
        let output = Command::new(&self.executable)
            .args(["debug", "models"])
            .output()
            .with_context(|| format!("query models from {}", self.executable.display()))?;
        if !output.status.success() {
            bail!(
                "Codex model discovery failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let catalog: ModelCatalog =
            serde_json::from_slice(&output.stdout).context("parse Codex model catalog")?;
        let models: Vec<_> = catalog
            .models
            .into_iter()
            .filter(|model| model.visibility == "list")
            .collect();
        if models.is_empty() {
            bail!("Codex returned no selectable models");
        }
        Ok(models)
    }

    fn run_passthrough<const N: usize>(&self, args: [&str; N]) -> Result<()> {
        let status = Command::new(&self.executable)
            .args(args)
            .status()
            .with_context(|| format!("run {}", self.executable.display()))?;
        if !status.success() {
            bail!("Codex exited with {status}");
        }
        Ok(())
    }

    pub fn generate(
        &self,
        root: &Path,
        patch: &str,
        history: &[String],
        count: u8,
        extra_instructions: Option<&str>,
    ) -> Result<Vec<CommitMessage>> {
        let temp = tempdir().context("create temporary Codex output directory")?;
        let schema_path = temp.path().join("schema.json");
        let output_path = temp.path().join("response.json");
        fs::write(&schema_path, schema(count))?;

        let mut command = Command::new(&self.executable);
        command
            .arg("exec")
            .args([
                "--ephemeral",
                "--sandbox",
                "read-only",
                "--color",
                "never",
                "--ignore-user-config",
                "--ignore-rules",
            ])
            .arg("--cd")
            .arg(root)
            .arg("--output-schema")
            .arg(&schema_path)
            .arg("--output-last-message")
            .arg(&output_path);
        if let Some(model) = &self.model {
            command.args(["--model", model]);
        }
        command
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        let mut child = command.spawn().with_context(|| {
            format!(
                "start {}; install Codex and run `gencommit auth login`",
                self.executable.display()
            )
        })?;
        let prompt = prompt(patch, history, count, extra_instructions);
        {
            let mut stdin = child.stdin.take().context("open Codex stdin")?;
            stdin.write_all(prompt.as_bytes())?;
        }
        let output = child.wait_with_output().context("wait for Codex")?;
        if !output.status.success() {
            let diagnostics = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Codex generation failed with {}: {}; check `gencommit auth status` and update the `model` in gencommit's config if compatibility changed",
                output.status,
                diagnostics.trim()
            );
        }

        let response = fs::read_to_string(&output_path).context("read Codex response")?;
        let generation: Generation =
            serde_json::from_str(&response).context("parse Codex response")?;
        if generation.variants.len() != usize::from(count) {
            bail!(
                "Codex returned {} variants, expected {count}",
                generation.variants.len()
            );
        }
        for message in &generation.variants {
            message.validate("Codex")?;
        }
        let mut rendered: Vec<_> = generation
            .variants
            .iter()
            .map(CommitMessage::render)
            .collect();
        rendered.sort();
        rendered.dedup();
        if rendered.len() != usize::from(count) {
            bail!("Codex returned duplicate commit messages");
        }
        Ok(generation.variants)
    }
}

pub(crate) fn schema(count: u8) -> String {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "variants": {
                "type": "array",
                "minItems": count,
                "maxItems": count,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "subject": { "type": "string" },
                        "body": { "type": ["string", "null"] }
                    },
                    "required": ["subject", "body"]
                }
            }
        },
        "required": ["variants"]
    })
    .to_string()
}

pub(crate) fn prompt(patch: &str, history: &[String], count: u8, extra: Option<&str>) -> String {
    let history = if history.is_empty() {
        "(no existing commits)".to_owned()
    } else {
        history
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        "Generate exactly {count} distinct Git commit message variants for the supplied patch.\n\
         Describe only changes evidenced by the patch. Follow the dominant recent-subject style when clear; otherwise use a concise imperative subject. Subjects must be one line and at most 100 characters. Add a body only when useful. Do not use tools or inspect the repository.\n\
         Additional instructions: {}\n\nRecent subjects:\n{}\n\nPatch:\n```diff\n{}\n```",
        extra.unwrap_or("(none)"),
        history,
        patch
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_optional_body() {
        let message = CommitMessage {
            subject: "Add CLI".into(),
            body: Some("Explain behavior".into()),
        };
        assert_eq!(message.render(), "Add CLI\n\nExplain behavior");
    }

    #[test]
    fn schema_has_exact_variant_count() {
        let value: serde_json::Value = serde_json::from_str(&schema(4)).unwrap();
        assert_eq!(value["properties"]["variants"]["minItems"], 4);
        assert_eq!(value["properties"]["variants"]["maxItems"], 4);
    }
}
