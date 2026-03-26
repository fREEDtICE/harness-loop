use std::{
    ffi::OsString,
    fs,
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

        info!(
            source_workspace = %source_workspace.display(),
            repo_root = %repo_root.display(),
            execution_root = %execution_root.display(),
            execution_workspace = %execution_workspace.display(),
            "preparing git worktree isolation"
        );

        if execution_root.exists() {
            bail!(
                "worktree target {} already exists",
                execution_root.display()
            );
        }

        if repo_has_head(&repo_root)? {
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
        } else {
            info!(
                repo_root = %repo_root.display(),
                execution_root = %execution_root.display(),
                "source repository has no HEAD; preparing isolated snapshot workspace instead"
            );
            prepare_snapshot_workspace(&repo_root, &execution_root, run_root)?;
        }

        info!(
            execution_root = %execution_root.display(),
            execution_workspace = %execution_workspace.display(),
            "prepared git worktree isolation"
        );

        Ok(PreparedWorkspace {
            source_workspace: source_workspace.to_path_buf(),
            execution_root,
            execution_workspace,
            isolation: WorkspaceIsolation::GitWorktree,
        })
    }
}

fn repo_has_head(repo_root: &Path) -> Result<bool> {
    let output = git_command(repo_root, ["rev-parse", "--verify", "HEAD"])?;
    Ok(output.status.success())
}

fn prepare_snapshot_workspace(
    repo_root: &Path,
    execution_root: &Path,
    run_root: &Path,
) -> Result<()> {
    let canonical_repo_root = canonicalize_existing_path(repo_root)?;
    let skipped_storage_root = run_root
        .parent()
        .map(canonicalize_existing_path)
        .transpose()?
        .and_then(|runs_dir| {
            if runs_dir.starts_with(&canonical_repo_root) && runs_dir != canonical_repo_root {
                Some(runs_dir.to_path_buf())
            } else {
                None
            }
        });
    let skipped_top_level_storage = skipped_storage_root
        .as_ref()
        .and_then(|runs_dir| runs_dir.strip_prefix(&canonical_repo_root).ok())
        .and_then(|relative| relative.components().next())
        .map(|component| component.as_os_str().to_os_string());
    copy_workspace_tree(
        &canonical_repo_root,
        &canonical_repo_root,
        execution_root,
        skipped_storage_root.as_deref(),
        skipped_top_level_storage.as_ref(),
    )?;
    run_git(execution_root, ["init"], "git init execution workspace")?;
    run_git(execution_root, ["add", "-A"], "git add execution workspace")?;
    run_git(
        execution_root,
        [
            "-c",
            "user.name=Codex Harness",
            "-c",
            "user.email=codex-harness@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "Harness snapshot bootstrap",
        ],
        "git commit execution workspace bootstrap",
    )?;
    Ok(())
}

fn copy_workspace_tree(
    copy_root: &Path,
    source: &Path,
    destination: &Path,
    skipped_storage_root: Option<&Path>,
    skipped_top_level_storage: Option<&OsString>,
) -> Result<()> {
    fs::create_dir_all(destination)
        .with_context(|| format!("failed to create {}", destination.display()))?;

    for entry in fs::read_dir(source)
        .with_context(|| format!("failed to read directory {}", source.display()))?
    {
        let entry = entry
            .with_context(|| format!("failed to read directory entry in {}", source.display()))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat {}", source_path.display()))?;
        let skip_top_level_storage = source == copy_root
            && skipped_top_level_storage.is_some_and(|name| entry.file_name() == *name);

        if entry.file_name() == ".git"
            || skip_top_level_storage
            || skipped_storage_root
                .is_some_and(|storage_root| source_path.starts_with(storage_root))
        {
            continue;
        }

        if file_type.is_dir() {
            copy_workspace_tree(
                copy_root,
                &source_path,
                &destination_path,
                skipped_storage_root,
                skipped_top_level_storage,
            )?;
            continue;
        }

        if file_type.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
            continue;
        }

        if file_type.is_symlink() {
            copy_symlink(&source_path, &destination_path)?;
            continue;
        }

        bail!(
            "unsupported workspace entry {} while preparing snapshot isolation",
            source_path.display()
        );
    }

    Ok(())
}

fn canonicalize_existing_path(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path)
        .map(normalize_path)
        .with_context(|| format!("failed to canonicalize {}", path.display()))
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target = fs::read_link(source)
        .with_context(|| format!("failed to read symlink {}", source.display()))?;
    std::os::unix::fs::symlink(&target, destination).with_context(|| {
        format!(
            "failed to create symlink {} -> {}",
            destination.display(),
            target.display()
        )
    })?;
    Ok(())
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target = fs::read_link(source)
        .with_context(|| format!("failed to read symlink {}", source.display()))?;
    let target_path = source
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(&target);
    let metadata = fs::metadata(&target_path).with_context(|| {
        format!(
            "failed to inspect symlink target {} from {}",
            target_path.display(),
            source.display()
        )
    })?;

    if metadata.is_dir() {
        std::os::windows::fs::symlink_dir(&target, destination)
    } else {
        std::os::windows::fs::symlink_file(&target, destination)
    }
    .with_context(|| {
        format!(
            "failed to create symlink {} -> {}",
            destination.display(),
            target.display()
        )
    })?;
    Ok(())
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

fn run_git<const N: usize>(workspace: &Path, args: [&str; N], description: &str) -> Result<()> {
    let output = git_command(workspace, args)?;
    if output.status.success() {
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "{description} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        stdout.trim(),
        stderr.trim()
    );
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

    #[test]
    fn git_worktree_falls_back_to_snapshot_when_repo_has_no_head() {
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

        let run_root = repo.join(".harness-runs").join("run");
        fs::create_dir_all(&run_root).expect("create run root");

        let prepared = WorkspaceManager::prepare(WorkspaceIsolation::GitWorktree, &repo, &run_root)
            .expect("prepare worktree");
        assert_ne!(prepared.execution_workspace, repo);
        assert!(prepared.execution_root.exists());
        assert!(prepared.execution_workspace.join("README.md").exists());
        assert!(!prepared.execution_root.join(".harness-runs").exists());

        let head_status = Command::new("git")
            .args([
                "-C",
                prepared
                    .execution_root
                    .to_str()
                    .expect("execution root path"),
                "rev-parse",
                "--verify",
                "HEAD",
            ])
            .status()
            .expect("git rev-parse");
        assert!(head_status.success());
    }
}
