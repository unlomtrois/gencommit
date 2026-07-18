use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use tempfile::{NamedTempFile, tempdir};

#[derive(Clone, Debug)]
pub enum Selection {
    All,
    Paths(Vec<PathBuf>),
}

impl Selection {
    fn append_pathspecs(&self, command: &mut Command) {
        if let Self::Paths(paths) = self {
            command.arg("--").args(paths);
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Self::All => "all changed paths".into(),
            Self::Paths(paths) => paths
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

pub struct Snapshot {
    pub patch: String,
    pub digest: [u8; 32],
    pub files: usize,
}

pub struct CommitResult {
    pub hash: String,
    pub subject: String,
}

pub struct Repository {
    root: PathBuf,
}

impl Repository {
    pub fn discover() -> Result<Self> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .context("run git; is it installed?")?;
        if !output.status.success() {
            bail!("not inside a Git worktree");
        }
        let root =
            String::from_utf8(output.stdout).context("Git returned a non-UTF-8 repository path")?;
        Ok(Self {
            root: PathBuf::from(root.trim()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn ensure_no_conflicts(&self, selection: &Selection) -> Result<()> {
        let mut command = self.git();
        command.args(["diff", "--name-only", "--diff-filter=U"]);
        selection.append_pathspecs(&mut command);
        let output = checked(command, "inspect merge conflicts")?;
        if !output.stdout.is_empty() {
            bail!("selected paths contain unresolved merge conflicts");
        }
        Ok(())
    }

    pub fn snapshot(&self, selection: &Selection) -> Result<Snapshot> {
        let temp = tempdir().context("create temporary Git index")?;
        let index = temp.path().join("index");
        let has_head = self.has_head()?;
        if has_head {
            let mut command = self.git_with_index(&index);
            command.args(["read-tree", "HEAD"]);
            checked(command, "prepare temporary Git index")?;
        }

        let mut add = self.git_with_index(&index);
        add.args(["add", "-A"]);
        selection.append_pathspecs(&mut add);
        checked(add, "collect selected changes")?;

        let mut diff = self.git_with_index(&index);
        diff.args([
            "diff",
            "--cached",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
        ]);
        if has_head {
            diff.arg("HEAD");
        }
        diff.arg("--");
        let output = checked(diff, "build selected patch")?;
        if output.stdout.is_empty() {
            bail!("no changes selected");
        }
        let patch =
            String::from_utf8(output.stdout).context("selected patch is not valid UTF-8")?;

        let mut names = self.git_with_index(&index);
        names.args(["diff", "--cached", "--name-only", "-z"]);
        if has_head {
            names.arg("HEAD");
        }
        names.arg("--");
        let output = checked(names, "count selected files")?;
        let files = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .count();
        let digest: [u8; 32] = Sha256::digest(patch.as_bytes()).into();
        Ok(Snapshot {
            patch,
            digest,
            files,
        })
    }

    pub fn recent_subjects(&self, limit: usize) -> Result<Vec<String>> {
        if !self.has_head()? {
            return Ok(Vec::new());
        }
        let mut command = self.git();
        command.args(["log", &format!("-{limit}"), "--format=%s"]);
        let output = checked(command, "read recent commit subjects")?;
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect())
    }

    pub fn stage_and_commit(&self, selection: &Selection, message: &str) -> Result<CommitResult> {
        let mut add = self.git();
        add.args(["add", "-A"]);
        selection.append_pathspecs(&mut add);
        checked(add, "stage selected changes")?;

        let mut message_file = NamedTempFile::new().context("create commit message file")?;
        std::io::Write::write_all(&mut message_file, message.as_bytes())?;
        let mut commit = self.git();
        commit.arg("commit").arg("-F").arg(message_file.path());
        if let Selection::Paths(paths) = selection {
            commit.arg("--only").arg("--").args(paths);
        }
        checked(commit, "create commit")?;

        let mut show = self.git();
        show.args(["show", "-s", "--format=%H%x00%s", "HEAD"]);
        let output = checked(show, "read new commit")?;
        let rendered =
            String::from_utf8(output.stdout).context("Git returned non-UTF-8 commit metadata")?;
        let (hash, subject) = rendered
            .trim_end()
            .split_once('\0')
            .context("parse new commit metadata")?;
        Ok(CommitResult {
            hash: hash[..12.min(hash.len())].to_owned(),
            subject: subject.to_owned(),
        })
    }

    fn has_head(&self) -> Result<bool> {
        let status = self
            .git()
            .args(["rev-parse", "--verify", "HEAD"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn git(&self) -> Command {
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.root);
        command
    }

    fn git_with_index(&self, index: &Path) -> Command {
        let mut command = self.git();
        command.env("GIT_INDEX_FILE", index);
        command
    }
}

fn checked(mut command: Command, context: &str) -> Result<Output> {
    let output = command.output().with_context(|| context.to_owned())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("{context}: {}", stderr.trim());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn repository() -> (tempfile::TempDir, Repository) {
        let temp = tempdir().unwrap();
        let root = temp.path().to_path_buf();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.name", "Gencommit Test"],
            vec!["config", "user.email", "gencommit@example.invalid"],
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(&root)
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        (temp, Repository { root })
    }

    #[test]
    fn snapshots_untracked_files_in_an_unborn_repository() {
        let (_temp, repo) = repository();
        fs::write(repo.root.join("new.txt"), "new contents\n").unwrap();

        let snapshot = repo.snapshot(&Selection::All).unwrap();

        assert_eq!(snapshot.files, 1);
        assert!(snapshot.patch.contains("new contents"));
        assert!(
            repo.git()
                .args(["diff", "--cached", "--quiet"])
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn explicit_commit_preserves_unrelated_staged_changes() {
        let (_temp, repo) = repository();
        fs::write(repo.root.join("selected.txt"), "base\n").unwrap();
        fs::write(repo.root.join("unrelated.txt"), "base\n").unwrap();
        let mut add_fixture = repo.git();
        add_fixture.args(["add", "."]);
        checked(add_fixture, "stage fixture").unwrap();
        let mut commit_fixture = repo.git();
        commit_fixture.args(["commit", "-qm", "Initial commit"]);
        checked(commit_fixture, "commit fixture").unwrap();

        fs::write(repo.root.join("selected.txt"), "selected change\n").unwrap();
        fs::write(repo.root.join("unrelated.txt"), "unrelated change\n").unwrap();
        let mut stage_unrelated = repo.git();
        stage_unrelated.args(["add", "unrelated.txt"]);
        checked(stage_unrelated, "stage unrelated fixture").unwrap();

        repo.stage_and_commit(
            &Selection::Paths(vec![PathBuf::from("selected.txt")]),
            "Update selected file",
        )
        .unwrap();

        let mut show = repo.git();
        show.args(["show", "--format=", "--name-only", "HEAD"]);
        let committed =
            String::from_utf8(checked(show, "show fixture commit").unwrap().stdout).unwrap();
        assert_eq!(committed.trim(), "selected.txt");

        let mut cached = repo.git();
        cached.args(["diff", "--cached", "--name-only"]);
        let staged =
            String::from_utf8(checked(cached, "show fixture index").unwrap().stdout).unwrap();
        assert_eq!(staged.trim(), "unrelated.txt");
    }
}
