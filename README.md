# gencommit

`gencommit` generates several Git commit-message candidates using your existing OpenAI Codex CLI login, lets you choose one in the terminal, and commits the selected changes after confirmation.

## Status

This is an early version. Review both the diff and generated message before approving a commit.

## Requirements

- Rust 1.85 or newer
- Git
- OpenAI Codex CLI authenticated with a ChatGPT account

ChatGPT subscriptions and OpenAI API billing are separate. `gencommit` delegates generation to Codex instead of accepting an OpenAI API key, and does not read or copy Codex authentication tokens.

## Install and authenticate

```sh
cargo install --path .
gencommit auth login
gencommit auth status
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
codex_executable = "codex"
model = "gpt-5.4-mini"
```

`gencommit` passes its model explicitly and ignores the global Codex model setting. This prevents an experimental model in `~/.codex/config.toml` from breaking commit generation. Override `model` here when your ChatGPT plan and installed Codex CLI support a different model.

Run `gencommit model` to fetch Codex's current model catalog, choose a model with the arrow-key interface, and save it to this file.
