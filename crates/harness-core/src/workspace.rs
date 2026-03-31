use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tracing::info;

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
                Self::prepare_git_workspace(&source_workspace, run_root)
            }
        }
    }

    fn prepare_git_workspace(
        source_workspace: &Path,
        _run_root: &Path,
    ) -> Result<PreparedWorkspace> {
        let repo_root = git_output(source_workspace, ["rev-parse", "--show-toplevel"])
            .context("failed to locate git repository root for git workspace execution")?;
        let repo_root = normalize_path(PathBuf::from(repo_root.trim()));

        info!(
            source_workspace = %source_workspace.display(),
            repo_root = %repo_root.display(),
            execution_workspace = %source_workspace.display(),
            "using source workspace directly for git-backed execution"
        );

        Ok(PreparedWorkspace {
            source_workspace: source_workspace.to_path_buf(),
            execution_root: source_workspace.to_path_buf(),
            execution_workspace: source_workspace.to_path_buf(),
            isolation: WorkspaceIsolation::GitWorktree,
        })
    }
}

fn git_output(source_workspace: &Path, args: [&str; 2]) -> Result<String> {
    let output = git_command(source_workspace, args)?;

    if !output.status.success() {
        bail!("git command failed with status {}", output.status);
    }

    String::from_utf8(output.stdout).context("git output was not valid UTF-8")
}

fn git_command<const N: usize>(workspace: &Path, args: [&str; N]) -> Result<Output> {
    Command::new("git")
        .args(["-C"])
        .arg(workspace)
        .args(args)
        .output()
        .context("failed to execute git command")
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
    fn git_worktree_uses_source_workspace_directly() {
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
        assert_eq!(prepared.execution_workspace, repo);
        assert_eq!(prepared.execution_root, repo);
        assert!(prepared.execution_workspace.join("README.md").exists());
    }

    #[test]
    fn git_worktree_uses_source_workspace_before_first_commit() {
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

        let run_root = repo.join(".loopsmith-runs").join("run");
        fs::create_dir_all(&run_root).expect("create run root");

        let prepared = WorkspaceManager::prepare(WorkspaceIsolation::GitWorktree, &repo, &run_root)
            .expect("prepare worktree");
        assert_eq!(prepared.execution_workspace, repo);
        assert_eq!(prepared.execution_root, repo);
        assert!(prepared.execution_workspace.join("README.md").exists());
        assert!(prepared.execution_workspace.join(".loopsmith-runs").exists());
    }
}
