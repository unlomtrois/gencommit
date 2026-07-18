# gencommit

`gencommit` generates several Git commit-message candidates using your existing Codex or Claude Code subscription login, lets you choose one in the terminal, and commits the selected changes after confirmation.

## Status

This is an early version. Review both the diff and generated message before approving a commit.

## Requirements

- Rust 1.85 or newer
- Git
- OpenAI Codex CLI authenticated with a ChatGPT account
- or Claude Code authenticated with a Claude.ai account

ChatGPT/Claude subscriptions and provider API billing are separate. `gencommit` delegates generation to the selected coding CLI instead of accepting an API key, and does not read or copy authentication tokens.

## Install and authenticate

Install the latest published crate:

```sh
cargo install gencommit
```

Alternatively, download the archive for your Linux or macOS architecture from the [GitHub Releases](https://github.com/unlomtrois/gencommit/releases) page and place `gencommit` on your `PATH`.

For development builds:

```sh
cargo install --path .
gencommit auth login
gencommit auth status
gencommit provider
gencommit model
```

## Usage

```sh
# Generate three variants for every changed path
gencommit

# Equivalent explicit form
gencommit --all

# Generate two variants, then stage and commit only these paths
gencommit -v 2 src/main.rs README.md
```

With explicit paths, unrelated staged changes remain staged and are excluded from the new commit. With no paths or `--all`, all worktree changes are staged and committed.

The selected patch is sent to Codex/OpenAI. Generation is read-only and ephemeral, but do not select files containing secrets you do not want to send to the model.

## Configuration

Optional configuration is read from `$XDG_CONFIG_HOME/gencommit/config.toml` (normally `~/.config/gencommit/config.toml`):

```toml
variants = 3
history_limit = 20
instructions = "Use Conventional Commits."
provider = "codex"
codex_executable = "codex"
model = "gpt-5.4-mini"
claude_executable = "claude"
claude_model = "haiku"
```

`gencommit` passes its model explicitly and ignores the global Codex model setting. This prevents an experimental model in `~/.codex/config.toml` from breaking commit generation. Override `model` here when your ChatGPT plan and installed Codex CLI support a different model.

Run `gencommit model` to fetch Codex's current model catalog, choose a model with the arrow-key interface, and save it to this file.

Run `gencommit provider` to switch between Codex/ChatGPT and Claude Code/Claude.ai. `gencommit auth` and `gencommit model` operate on the selected provider. Claude generation uses safe mode, disables built-in tools, and does not persist sessions.
