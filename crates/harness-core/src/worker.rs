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
}

pub fn render_worker_prompt<T: Serialize>(
    context: &WorkerContext,
    feature: Option<&FeatureLayout>,
    stage: crate::domain::WorkerStage,
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

    Ok(format!(
        "{template}\n\nHarness Context:\n- run_id: {}\n- stage: {}\n- workspace: {}\n- request_artifact: {}\n- plan_artifact: {}\n- runtime_plan_artifact: {}\n{feature_context}\nReturn Format:\nReturn only JSON matching this schema:\n{schema}\n\nPayload:\n{payload}\n",
        context.run_id,
        stage.as_str(),
        context.workspace.display(),
        context.layout.request_file.display(),
        context.layout.plan_file.display(),
        context.layout.runtime_plan_file.display(),
    ))
}

#[async_trait]
pub trait WorkerAdapter: Send + Sync {
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
