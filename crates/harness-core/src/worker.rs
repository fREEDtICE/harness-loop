use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Serialize;
use uuid::Uuid;

use crate::{
    artifacts::{FeatureLayout, RunLayout, StageArtifactSet},
    discovery::{DiscoveryArtifactSet, WorkspaceDiscoveryRequest},
    domain::{
        BuilderHandoff, EvaluationRequest, FeatureContract, PlanningRequest, QaReport, WorkerResult,
    },
};

#[derive(Debug, Clone)]
pub struct WorkerContext {
    pub run_id: Uuid,
    pub workspace: PathBuf,
    pub layout: RunLayout,
    pub planner_prompt: PathBuf,
    pub builder_prompt: PathBuf,
    pub evaluator_prompt: PathBuf,
    pub planner_schema: PathBuf,
    pub builder_schema: PathBuf,
    pub qa_schema: PathBuf,
    pub workspace_profile_artifact: Option<PathBuf>,
    pub workspace_profile_context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub workspace: PathBuf,
    pub discovery_prompt: PathBuf,
    pub workspace_profile_schema: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryWorkerResult {
    pub status: crate::domain::WorkerStatus,
    pub command: Vec<String>,
    pub prompt_file: PathBuf,
    pub output_file: PathBuf,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub notes: Vec<String>,
    pub session_id: Option<String>,
}

pub fn render_worker_prompt<T: Serialize>(
    context: &WorkerContext,
    feature: Option<&FeatureLayout>,
    stage_label: &str,
    template_path: &Path,
    schema_path: &Path,
    payload: &T,
) -> Result<String> {
    let template = fs::read_to_string(template_path)
        .with_context(|| format!("failed to read {}", template_path.display()))?;
    let schema = fs::read_to_string(schema_path)
        .with_context(|| format!("failed to read {}", schema_path.display()))?;
    let payload = serde_json::to_string_pretty(payload).context("failed to serialize payload")?;

    let feature_context = feature.map_or_else(
        || "Feature Context:\n- feature_root: -\n- contract_artifact: -\n- builder_handoff_artifact: -\n- qa_artifact: -\n".to_string(),
        |feature| {
            format!(
                "Feature Context:\n- feature_root: {}\n- contract_artifact: {}\n- builder_handoff_artifact: {}\n- qa_artifact: {}\n",
                feature.root.display(),
                feature.contract_file.display(),
                feature.builder_handoff_file.display(),
                feature.qa_report_file.display(),
            )
        },
    );
    let workspace_profile_context = context.workspace_profile_artifact.as_ref().map_or_else(
        || {
            "Workspace Profile Context:\n- workspace_profile_artifact: -\n- workspace_profile_summary: -\n".to_string()
        },
        |path| {
            let summary = context
                .workspace_profile_context
                .as_deref()
                .unwrap_or("No compact profile summary was available.");
            format!(
                "Workspace Profile Context:\n- workspace_profile_artifact: {}\n{}\n",
                path.display(),
                summary
            )
        },
    );

    Ok(format!(
        "{template}\n\nHarness Context:\n- run_id: {}\n- stage: {}\n- workspace: {}\n- request_artifact: {}\n- plan_artifact: {}\n- runtime_plan_artifact: {}\n{feature_context}{workspace_profile_context}\nReturn Format:\nReturn only JSON matching this schema:\n{schema}\n\nPayload:\n{payload}\n",
        context.run_id,
        stage_label,
        context.workspace.display(),
        context.layout.request_file.display(),
        context.layout.plan_file.display(),
        context.layout.runtime_plan_file.display(),
    ))
}

pub fn render_discovery_prompt(
    context: &DiscoveryContext,
    schema_path: &Path,
    payload: &WorkspaceDiscoveryRequest,
) -> Result<String> {
    let template = fs::read_to_string(&context.discovery_prompt).with_context(|| {
        format!(
            "failed to read discovery prompt {}",
            context.discovery_prompt.display()
        )
    })?;
    let schema = fs::read_to_string(schema_path)
        .with_context(|| format!("failed to read {}", schema_path.display()))?;
    let payload = serde_json::to_string_pretty(payload).context("failed to serialize payload")?;

    Ok(format!(
        "{template}\n\nDiscovery Context:\n- stage: discover\n- workspace: {}\n- output_schema: {}\n\nReturn Format:\nReturn only JSON matching this schema:\n{schema}\n\nPayload:\n{payload}\n",
        context.workspace.display(),
        schema_path.display(),
    ))
}

#[async_trait]
pub trait WorkerAdapter: Send + Sync {
    async fn discover(
        &self,
        context: &DiscoveryContext,
        artifacts: &DiscoveryArtifactSet,
        request: &WorkspaceDiscoveryRequest,
    ) -> Result<DiscoveryWorkerResult>;

    async fn plan(
        &self,
        context: &WorkerContext,
        artifacts: &StageArtifactSet,
        request: &PlanningRequest,
    ) -> Result<WorkerResult>;

    async fn build(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
    ) -> Result<WorkerResult>;

    async fn evaluate(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        request: &EvaluationRequest,
    ) -> Result<WorkerResult>;

    async fn repair(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
        builder_handoff: &BuilderHandoff,
        qa_report: &QaReport,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult>;
}

#[async_trait]
impl<T> WorkerAdapter for Box<T>
where
    T: WorkerAdapter + ?Sized,
{
    async fn discover(
        &self,
        context: &DiscoveryContext,
        artifacts: &DiscoveryArtifactSet,
        request: &WorkspaceDiscoveryRequest,
    ) -> Result<DiscoveryWorkerResult> {
        (**self).discover(context, artifacts, request).await
    }

    async fn plan(
        &self,
        context: &WorkerContext,
        artifacts: &StageArtifactSet,
        request: &PlanningRequest,
    ) -> Result<WorkerResult> {
        (**self).plan(context, artifacts, request).await
    }

    async fn build(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
    ) -> Result<WorkerResult> {
        (**self).build(context, feature, artifacts, contract).await
    }

    async fn evaluate(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        request: &EvaluationRequest,
    ) -> Result<WorkerResult> {
        (**self)
            .evaluate(context, feature, artifacts, request)
            .await
    }

    async fn repair(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &FeatureContract,
        builder_handoff: &BuilderHandoff,
        qa_report: &QaReport,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult> {
        (**self)
            .repair(
                context,
                feature,
                artifacts,
                contract,
                builder_handoff,
                qa_report,
                previous_session_id,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use tempfile::tempdir;
    use uuid::Uuid;

    use crate::{artifacts::RunLayout, domain::PlanningRequest};

    use super::{WorkerContext, render_worker_prompt};

    #[test]
    fn render_worker_prompt_includes_workspace_profile_context() {
        let temp = tempdir().expect("tempdir");
        let prompt_path = temp.path().join("planner.md");
        let schema_path = temp.path().join("planner-output.json");
        fs::write(&prompt_path, "planner prompt").expect("write prompt");
        fs::write(&schema_path, "{\"type\":\"object\"}").expect("write schema");

        let layout = RunLayout {
            root: temp.path().join("run"),
            inputs_dir: temp.path().join("run/inputs"),
            prompt_inputs_dir: temp.path().join("run/inputs/prompts"),
            workspace_profile_file: temp.path().join("run/inputs/workspace-profile.json"),
            planner_prompt_file: temp.path().join("run/inputs/prompts/planner.md"),
            builder_prompt_file: temp.path().join("run/inputs/prompts/builder.md"),
            evaluator_prompt_file: temp.path().join("run/inputs/prompts/evaluator.md"),
            launch_file: temp.path().join("run/launch.json"),
            request_file: temp.path().join("run/request.md"),
            plan_file: temp.path().join("run/plan.json"),
            runtime_plan_file: temp.path().join("run/runtime-plan.json"),
            state_file: temp.path().join("run/run-state.json"),
            manifest_file: temp.path().join("run/manifest.json"),
            features_dir: temp.path().join("run/features"),
            worker_dir: temp.path().join("run/worker"),
        };
        let context = WorkerContext {
            run_id: Uuid::nil(),
            workspace: temp.path().join("workspace"),
            layout,
            planner_prompt: prompt_path.clone(),
            builder_prompt: PathBuf::new(),
            evaluator_prompt: PathBuf::new(),
            planner_schema: schema_path.clone(),
            builder_schema: PathBuf::new(),
            qa_schema: PathBuf::new(),
            workspace_profile_artifact: Some(temp.path().join("run/inputs/workspace-profile.json")),
            workspace_profile_context: Some(
                "- summary: Workspace profile summary\n- layering_rules: UI depends on service."
                    .to_string(),
            ),
        };

        let prompt = render_worker_prompt(
            &context,
            None,
            "plan",
            &prompt_path,
            &schema_path,
            &PlanningRequest {
                user_request: "Build a harness".to_string(),
                feature_limit: 1,
                feature_limit_is_hard: true,
                service_names: vec!["web".to_string()],
                verification_commands: vec![vec!["cargo".to_string(), "test".to_string()]],
            },
        )
        .expect("render prompt");

        assert!(prompt.contains("Workspace Profile Context:"));
        assert!(prompt.contains("workspace-profile.json"));
        assert!(prompt.contains("Workspace profile summary"));
        assert!(prompt.contains("layering_rules: UI depends on service."));
    }
}
