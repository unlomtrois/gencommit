use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::codex::{CommitMessage, Generation, Model, prompt, schema};

pub struct Claude {
    executable: PathBuf,
    model: Option<String>,
}

impl Claude {
    pub fn new(executable: PathBuf, model: Option<String>) -> Self {
        Self { executable, model }
    }

    pub fn login(&self) -> Result<()> {
        self.passthrough(["auth", "login"])
    }

    pub fn login_status(&self) -> Result<()> {
        self.passthrough(["auth", "status"])
    }

    pub fn models(&self) -> Vec<Model> {
        [
            (
                "haiku",
                "Haiku",
                "Fastest model for lightweight commit-message generation.",
            ),
            (
                "sonnet",
                "Sonnet",
                "Balanced model for nuanced or larger changes.",
            ),
            ("opus", "Opus", "Most capable model for complex changes."),
            (
                "fable",
                "Fable",
                "Current Fable model alias exposed by Claude Code.",
            ),
        ]
        .into_iter()
        .map(|(slug, display_name, description)| Model {
            slug: slug.into(),
            display_name: display_name.into(),
            description: description.into(),
            visibility: "list".into(),
        })
        .collect()
    }

    pub fn generate(
        &self,
        root: &Path,
        patch: &str,
        history: &[String],
        count: u8,
        extra_instructions: Option<&str>,
    ) -> Result<Vec<CommitMessage>> {
        let mut command = Command::new(&self.executable);
        command.current_dir(root).args([
            "--safe-mode",
            "--print",
            "--tools",
            "",
            "--no-session-persistence",
            "--output-format",
            "json",
            "--json-schema",
            &schema(count),
        ]);
        if let Some(model) = &self.model {
            command.args(["--model", model]);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().with_context(|| {
            format!(
                "start {}; install Claude Code and run `gencommit auth login`",
                self.executable.display()
            )
        })?;
        {
            let mut stdin = child.stdin.take().context("open Claude stdin")?;
            stdin.write_all(prompt(patch, history, count, extra_instructions).as_bytes())?;
        }
        let output = child.wait_with_output().context("wait for Claude")?;
        if !output.status.success() {
            bail!(
                "Claude generation failed with {}: {}; check `gencommit auth status` and the selected model",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        let generation = parse_generation(&output.stdout)?;
        if generation.variants.len() != usize::from(count) {
            bail!(
                "Claude returned {} variants, expected {count}",
                generation.variants.len()
            );
        }
        for message in &generation.variants {
            message.validate("Claude")?;
        }
        let mut unique: Vec<_> = generation
            .variants
            .iter()
            .map(CommitMessage::render)
            .collect();
        unique.sort();
        unique.dedup();
        if unique.len() != usize::from(count) {
            bail!("Claude returned duplicate commit messages");
        }
        Ok(generation.variants)
    }

    fn passthrough<const N: usize>(&self, args: [&str; N]) -> Result<()> {
        let status = Command::new(&self.executable)
            .args(args)
            .status()
            .with_context(|| format!("run {}", self.executable.display()))?;
        if !status.success() {
            bail!("Claude exited with {status}");
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct ClaudeResult {
    structured_output: Option<Generation>,
}

fn parse_generation(bytes: &[u8]) -> Result<Generation> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).context("parse Claude response")?;
    let result = if let Some(events) = value.as_array() {
        events
            .iter()
            .rev()
            .find(|event| event.get("type").and_then(|kind| kind.as_str()) == Some("result"))
            .context("Claude response did not contain a result event")?
            .clone()
    } else {
        value
    };
    serde_json::from_value::<ClaudeResult>(result)
        .context("parse Claude result")?
        .structured_output
        .context("Claude result did not contain structured output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_result_response() {
        let response = br#"{
            "type": "result",
            "structured_output": {
                "variants": [{"subject": "Add provider", "body": null}]
            }
        }"#;

        let generation = parse_generation(response).unwrap();
        assert_eq!(generation.variants[0].subject, "Add provider");
    }

    #[test]
    fn parses_event_array_response() {
        let response = br#"[
            {"type": "system"},
            {
                "type": "result",
                "structured_output": {
                    "variants": [{"subject": "Use Claude", "body": "Add a provider backend."}]
                }
            }
        ]"#;

        let generation = parse_generation(response).unwrap();
        assert_eq!(
            generation.variants[0].render(),
            "Use Claude\n\nAdd a provider backend."
        );
    }
}
