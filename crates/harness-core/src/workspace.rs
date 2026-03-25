use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::paths::normalize_path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceIsolation {
    #[default]
    Direct,
    GitWorktree,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedWorkspace {
    pub source_workspace: PathBuf,
    pub execution_root: PathBuf,
    pub execution_workspace: PathBuf,
    pub isolation: WorkspaceIsolation,
}

pub struct WorkspaceManager;

impl WorkspaceManager {
    pub fn prepare(
        isolation: WorkspaceIsolation,
        source_workspace: &Path,
        run_root: &Path,
    ) -> Result<PreparedWorkspace> {
        let source_workspace = normalize_path(source_workspace.to_path_buf());

        match isolation {
            WorkspaceIsolation::Direct => Ok(PreparedWorkspace {
                source_workspace: source_workspace.clone(),
                execution_root: source_workspace.clone(),
                execution_workspace: source_workspace,
                isolation,
            }),
            WorkspaceIsolation::GitWorktree => {
                Self::prepare_git_worktree(&source_workspace, run_root)
            }
        }
    }

    fn prepare_git_worktree(source_workspace: &Path, run_root: &Path) -> Result<PreparedWorkspace> {
        let repo_root = git_output(source_workspace, ["rev-parse", "--show-toplevel"])
            .context("failed to locate git repository root for worktree isolation")?;
        let repo_root = normalize_path(PathBuf::from(repo_root.trim()));

        let relative_workspace = source_workspace
            .strip_prefix(&repo_root)
            .unwrap_or_else(|_| Path::new(""));
        let execution_root = normalize_path(run_root.join("workspace"));
        let execution_workspace = if relative_workspace.as_os_str().is_empty() {
            execution_root.clone()
        } else {
            normalize_path(execution_root.join(relative_workspace))
        };

        if execution_root.exists() {
            bail!(
                "worktree target {} already exists",
                execution_root.display()
            );
        }

        let status = Command::new("git")
            .args([
                "-C",
                repo_root.to_str().context("repo root is not valid UTF-8")?,
                "worktree",
                "add",
                "--detach",
                execution_root
                    .to_str()
                    .context("execution root is not valid UTF-8")?,
                "HEAD",
            ])
            .status()
            .context("failed to execute git worktree add")?;

        if !status.success() {
            bail!("git worktree add failed with status {status}");
        }

        Ok(PreparedWorkspace {
            source_workspace: source_workspace.to_path_buf(),
            execution_root,
            execution_workspace,
            isolation: WorkspaceIsolation::GitWorktree,
        })
    }
}

fn git_output(source_workspace: &Path, args: [&str; 2]) -> Result<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(source_workspace)
        .args(args)
        .output()
        .context("failed to execute git command")?;

    if !output.status.success() {
        bail!("git command failed with status {}", output.status);
    }

    String::from_utf8(output.stdout).context("git output was not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command};

    use tempfile::tempdir;

    use super::{WorkspaceIsolation, WorkspaceManager};

    #[test]
    fn direct_workspace_keeps_source_path() {
        let temp = tempdir().expect("tempdir");
        let source = temp.path().join("repo");
        fs::create_dir_all(&source).expect("create source");

        let prepared = WorkspaceManager::prepare(WorkspaceIsolation::Direct, &source, temp.path())
            .expect("prepare");
        assert_eq!(prepared.source_workspace, source);
        assert_eq!(prepared.execution_workspace, prepared.source_workspace);
    }

    #[test]
    fn git_worktree_creates_isolated_workspace() {
        let temp = tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo");
        fs::write(repo.join("README.md"), "hello\n").expect("write repo file");

        assert!(
            Command::new("git")
                .args(["init", repo.to_str().expect("repo path")])
                .status()
                .expect("git init")
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "-C",
                    repo.to_str().expect("repo path"),
                    "config",
                    "user.email",
                    "test@example.com"
                ])
                .status()
                .expect("git config")
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "-C",
                    repo.to_str().expect("repo path"),
                    "config",
                    "user.name",
                    "Test"
                ])
                .status()
                .expect("git config")
                .success()
        );
        assert!(
            Command::new("git")
                .args(["-C", repo.to_str().expect("repo path"), "add", "."])
                .status()
                .expect("git add")
                .success()
        );
        assert!(
            Command::new("git")
                .args([
                    "-C",
                    repo.to_str().expect("repo path"),
                    "commit",
                    "-m",
                    "init"
                ])
                .status()
                .expect("git commit")
                .success()
        );

        let run_root = temp.path().join("run");
        fs::create_dir_all(&run_root).expect("create run root");

        let prepared = WorkspaceManager::prepare(WorkspaceIsolation::GitWorktree, &repo, &run_root)
            .expect("prepare worktree");
        assert_ne!(prepared.execution_workspace, repo);
        assert!(prepared.execution_root.exists());
        assert!(prepared.execution_workspace.join("README.md").exists());
    }
}
