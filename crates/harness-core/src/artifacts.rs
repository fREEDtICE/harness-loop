use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::{Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::{domain::WorkerStage, paths::normalize_path};

#[derive(Debug, Clone)]
pub struct FileArtifactStore {
    base_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunLayout {
    pub root: PathBuf,
    pub inputs_dir: PathBuf,
    pub prompt_inputs_dir: PathBuf,
    pub planner_prompt_file: PathBuf,
    pub builder_prompt_file: PathBuf,
    pub evaluator_prompt_file: PathBuf,
    pub launch_file: PathBuf,
    pub request_file: PathBuf,
    pub plan_file: PathBuf,
    pub runtime_plan_file: PathBuf,
    pub state_file: PathBuf,
    pub manifest_file: PathBuf,
    pub features_dir: PathBuf,
    pub worker_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeatureLayout {
    pub root: PathBuf,
    pub contract_file: PathBuf,
    pub builder_handoff_file: PathBuf,
    pub qa_report_file: PathBuf,
    pub runtime_dir: PathBuf,
    pub worker_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerificationArtifactSet {
    pub attempt: usize,
    pub root: PathBuf,
    pub report_file: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenshotArtifactSet {
    pub attempt: usize,
    pub root: PathBuf,
    pub report_file: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct StageArtifactSet {
    pub stage: WorkerStage,
    pub attempt: usize,
    pub prompt_file: PathBuf,
    pub output_file: PathBuf,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub result_file: PathBuf,
}

impl FileArtifactStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    pub fn initialize(&self, run_id: Uuid) -> Result<RunLayout> {
        let root = self.base_dir.join(run_id.to_string());
        let inputs_dir = root.join("inputs");
        let prompt_inputs_dir = inputs_dir.join("prompts");
        let features_dir = root.join("features");
        let worker_dir = root.join("worker");
        let logs_dir = worker_dir.join("logs");
        let prompts_dir = worker_dir.join("prompts");
        let outputs_dir = worker_dir.join("outputs");

        for dir in [
            &root,
            &inputs_dir,
            &prompt_inputs_dir,
            &features_dir,
            &worker_dir,
            &logs_dir,
            &prompts_dir,
            &outputs_dir,
        ] {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create directory {}", dir.display()))?;
        }

        Ok(RunLayout {
            inputs_dir: inputs_dir.clone(),
            prompt_inputs_dir: prompt_inputs_dir.clone(),
            planner_prompt_file: prompt_inputs_dir.join("planner.md"),
            builder_prompt_file: prompt_inputs_dir.join("builder.md"),
            evaluator_prompt_file: prompt_inputs_dir.join("evaluator.md"),
            launch_file: root.join("launch.json"),
            request_file: root.join("request.md"),
            plan_file: root.join("plan.json"),
            runtime_plan_file: root.join("runtime-plan.json"),
            state_file: root.join("run-state.json"),
            manifest_file: root.join("manifest.json"),
            features_dir,
            worker_dir,
            root,
        })
    }

    pub fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(value).context("failed to serialize json")?;
        fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn read_json<T: DeserializeOwned>(&self, path: &Path) -> Result<T> {
        let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
        serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse {}", path.display()))
    }

    pub fn write_text(&self, path: &Path, value: &str) -> Result<()> {
        fs::write(path, value).with_context(|| format!("failed to write {}", path.display()))
    }
}

impl RunLayout {
    pub fn stage_artifacts(&self, stage: WorkerStage, attempt: usize) -> StageArtifactSet {
        let stem = format!("{}-{:02}", stage.as_str(), attempt);

        StageArtifactSet {
            stage,
            attempt,
            prompt_file: self.worker_dir.join("prompts").join(format!("{stem}.md")),
            output_file: self
                .worker_dir
                .join("outputs")
                .join(format!("{stem}-last-message.json")),
            stdout_log: self
                .worker_dir
                .join("logs")
                .join(format!("{stem}-stdout.log")),
            stderr_log: self
                .worker_dir
                .join("logs")
                .join(format!("{stem}-stderr.log")),
            result_file: self.worker_dir.join(format!("{stem}-result.json")),
        }
    }

    pub fn feature_layout(&self, index: usize, feature_id: &str) -> Result<FeatureLayout> {
        let safe_id = sanitize_for_path(feature_id);
        let root = normalize_path(
            self.features_dir
                .join(format!("{:02}-{safe_id}", index + 1)),
        );
        let worker_dir = root.join("worker");
        let runtime_dir = root.join("runtime");
        let logs_dir = worker_dir.join("logs");
        let prompts_dir = worker_dir.join("prompts");
        let outputs_dir = worker_dir.join("outputs");

        for dir in [
            &root,
            &worker_dir,
            &runtime_dir,
            &logs_dir,
            &prompts_dir,
            &outputs_dir,
        ] {
            fs::create_dir_all(dir)
                .with_context(|| format!("failed to create directory {}", dir.display()))?;
        }

        Ok(FeatureLayout {
            contract_file: root.join("feature-contract.json"),
            builder_handoff_file: root.join("builder-handoff.json"),
            qa_report_file: root.join("qa-report.json"),
            runtime_dir,
            root,
            worker_dir,
        })
    }
}

impl FeatureLayout {
    pub fn stage_artifacts(&self, stage: WorkerStage, attempt: usize) -> StageArtifactSet {
        let stem = format!("{}-{:02}", stage.as_str(), attempt);

        StageArtifactSet {
            stage,
            attempt,
            prompt_file: self.worker_dir.join("prompts").join(format!("{stem}.md")),
            output_file: self
                .worker_dir
                .join("outputs")
                .join(format!("{stem}-last-message.json")),
            stdout_log: self
                .worker_dir
                .join("logs")
                .join(format!("{stem}-stdout.log")),
            stderr_log: self
                .worker_dir
                .join("logs")
                .join(format!("{stem}-stderr.log")),
            result_file: self.worker_dir.join(format!("{stem}-result.json")),
        }
    }

    pub fn verification_artifacts(&self, attempt: usize) -> Result<VerificationArtifactSet> {
        let root = self
            .runtime_dir
            .join("verification")
            .join(format!("evaluate-{attempt:02}"));
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create directory {}", root.display()))?;

        Ok(VerificationArtifactSet {
            attempt,
            report_file: root.join("report.json"),
            root,
        })
    }

    pub fn screenshot_artifacts(&self, attempt: usize) -> Result<ScreenshotArtifactSet> {
        let root = self
            .runtime_dir
            .join("screenshots")
            .join(format!("evaluate-{attempt:02}"));
        fs::create_dir_all(&root)
            .with_context(|| format!("failed to create directory {}", root.display()))?;

        Ok(ScreenshotArtifactSet {
            attempt,
            report_file: root.join("report.json"),
            root,
        })
    }
}

fn sanitize_for_path(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            sanitized.push(ch.to_ascii_lowercase());
        } else if !sanitized.ends_with('-') {
            sanitized.push('-');
        }
    }

    sanitized.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use uuid::Uuid;

    use super::{FileArtifactStore, sanitize_for_path};

    #[test]
    fn sanitize_feature_path() {
        assert_eq!(sanitize_for_path("Feature 01 / Auth"), "feature-01-auth");
    }

    #[test]
    fn feature_layout_creates_per_feature_directories() {
        let temp = tempdir().expect("tempdir");
        let store = FileArtifactStore::new(temp.path().join("runs"));
        let layout = store.initialize(Uuid::nil()).expect("run layout");
        let feature = layout
            .feature_layout(0, "Feature 01 / Auth")
            .expect("feature layout");

        assert!(feature.root.exists());
        assert!(feature.worker_dir.join("prompts").exists());
    }
}
