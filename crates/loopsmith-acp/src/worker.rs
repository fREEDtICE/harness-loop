use std::{fs, path::Path};

use anyhow::{Context, Result};
use async_trait::async_trait;
use loopsmith_core::{
    artifacts::{FeatureLayout, StageArtifactSet},
    config::AcpWorkerConfig,
    discovery::{DiscoveryArtifactSet, WorkspaceDiscoveryRequest},
    domain::{
        BuilderHandoff, EvaluationRequest, PlannerConversationRequest, PlanningRequest, QaReport,
        WorkerResult, WorkerStatus,
    },
    worker::{
        DiscoveryContext, DiscoveryWorkerResult, PlannerConversationArtifactSet,
        PlannerConversationContext, PlannerConversationWorkerResult, WorkerAdapter, WorkerContext,
        render_discovery_prompt, render_planner_conversation_prompt, render_worker_prompt,
    },
};
use serde::Serialize;
use tracing::info;

use crate::client::run_acp_prompt;

pub struct AcpWorker {
    config: AcpWorkerConfig,
}

impl AcpWorker {
    pub fn new(config: AcpWorkerConfig) -> Self {
        Self { config }
    }

    fn full_command(&self) -> Vec<String> {
        let mut cmd = self.config.command.clone();
        cmd.extend(self.config.args.iter().cloned());
        cmd
    }

    async fn run_acp_stage(
        &self,
        workspace: &Path,
        prompt: &str,
        prompt_file: &Path,
        output_file: &Path,
        stdout_log: &Path,
        stderr_log: &Path,
        stage_label: &str,
        attempt: usize,
        previous_session_id: Option<&str>,
    ) -> Result<(Option<String>, Option<Vec<u8>>)> {
        let command = self.full_command();
        info!(
            stage = stage_label,
            attempt,
            command = command.join(" "),
            prompt_file = %prompt_file.display(),
            output_file = %output_file.display(),
            "starting ACP worker stage"
        );

        fs::write(prompt_file, prompt)
            .with_context(|| format!("failed to write {}", prompt_file.display()))?;

        fs::write(stderr_log, "")
            .with_context(|| format!("failed to create {}", stderr_log.display()))?;

        let session = run_acp_prompt(
            &command,
            workspace,
            prompt,
            stdout_log,
            previous_session_id,
            self.config.resume_sessions,
        )
        .await?;

        info!(
            stage = stage_label,
            attempt,
            session_id = %session.session_id,
            stop_reason = ?session.stop_reason,
            "completed ACP worker stage"
        );

        let json_output = extract_json_from_output(&session.output);

        if let Some(ref json_bytes) = json_output {
            fs::write(output_file, json_bytes)
                .with_context(|| format!("failed to write {}", output_file.display()))?;
        } else if !session.output.trim().is_empty() {
            fs::write(output_file, &session.output)
                .with_context(|| format!("failed to write {}", output_file.display()))?;
        }

        Ok((Some(session.session_id), json_output))
    }

    async fn run_discovery(
        &self,
        context: &DiscoveryContext,
        artifacts: &DiscoveryArtifactSet,
        request: &WorkspaceDiscoveryRequest,
    ) -> Result<DiscoveryWorkerResult> {
        let prompt = render_discovery_prompt(context, &context.workspace_profile_schema, request)?;
        let command = self.full_command();

        let (session_id, _) = self
            .run_acp_stage(
                &context.workspace,
                &prompt,
                &artifacts.prompt_file,
                &artifacts.output_file,
                &artifacts.stdout_log,
                &artifacts.stderr_log,
                "discover",
                1,
                None,
            )
            .await?;

        Ok(DiscoveryWorkerResult {
            status: WorkerStatus::Executed,
            command,
            prompt_file: artifacts.prompt_file.clone(),
            output_file: artifacts.output_file.clone(),
            stdout_log: artifacts.stdout_log.clone(),
            stderr_log: artifacts.stderr_log.clone(),
            notes: vec!["ACP execution completed.".to_string()],
            session_id,
        })
    }

    async fn run_exec_stage<T: Serialize>(
        &self,
        context: &WorkerContext,
        feature: Option<&FeatureLayout>,
        artifacts: &StageArtifactSet,
        template_path: &Path,
        schema_path: &Path,
        payload: &T,
    ) -> Result<WorkerResult> {
        let prompt = render_worker_prompt(
            context,
            feature,
            artifacts.stage.as_str(),
            template_path,
            schema_path,
            payload,
        )?;
        let command = self.full_command();

        let (session_id, _) = self
            .run_acp_stage(
                &context.workspace,
                &prompt,
                &artifacts.prompt_file,
                &artifacts.output_file,
                &artifacts.stdout_log,
                &artifacts.stderr_log,
                artifacts.stage.as_str(),
                artifacts.attempt,
                None,
            )
            .await?;

        Ok(WorkerResult {
            stage: artifacts.stage,
            status: WorkerStatus::Executed,
            command,
            prompt_file: artifacts.prompt_file.clone(),
            output_file: artifacts.output_file.clone(),
            stdout_log: artifacts.stdout_log.clone(),
            stderr_log: artifacts.stderr_log.clone(),
            notes: vec!["ACP execution completed.".to_string()],
            session_id,
        })
    }

    async fn run_repair<T: Serialize>(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        template_path: &Path,
        schema_path: &Path,
        payload: &T,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult> {
        let prompt = render_worker_prompt(
            context,
            Some(feature),
            artifacts.stage.as_str(),
            template_path,
            schema_path,
            payload,
        )?;
        let command = self.full_command();

        let (session_id, _) = self
            .run_acp_stage(
                &context.workspace,
                &prompt,
                &artifacts.prompt_file,
                &artifacts.output_file,
                &artifacts.stdout_log,
                &artifacts.stderr_log,
                artifacts.stage.as_str(),
                artifacts.attempt,
                previous_session_id,
            )
            .await?;

        Ok(WorkerResult {
            stage: artifacts.stage,
            status: WorkerStatus::Executed,
            command,
            prompt_file: artifacts.prompt_file.clone(),
            output_file: artifacts.output_file.clone(),
            stdout_log: artifacts.stdout_log.clone(),
            stderr_log: artifacts.stderr_log.clone(),
            notes: vec!["ACP execution completed.".to_string()],
            session_id,
        })
    }
}

#[async_trait]
impl WorkerAdapter for AcpWorker {
    async fn discover(
        &self,
        context: &DiscoveryContext,
        artifacts: &DiscoveryArtifactSet,
        request: &WorkspaceDiscoveryRequest,
    ) -> Result<DiscoveryWorkerResult> {
        self.run_discovery(context, artifacts, request).await
    }

    async fn consult_planner(
        &self,
        context: &PlannerConversationContext,
        artifacts: &PlannerConversationArtifactSet,
        request: &PlannerConversationRequest,
    ) -> Result<PlannerConversationWorkerResult> {
        let prompt = render_planner_conversation_prompt(context, request)?;
        let command = self.full_command();

        let (session_id, _) = self
            .run_acp_stage(
                &context.workspace,
                &prompt,
                &artifacts.prompt_file,
                &artifacts.output_file,
                &artifacts.stdout_log,
                &artifacts.stderr_log,
                "planner_consult",
                1,
                None,
            )
            .await?;

        Ok(PlannerConversationWorkerResult {
            status: WorkerStatus::Executed,
            command,
            prompt_file: artifacts.prompt_file.clone(),
            output_file: artifacts.output_file.clone(),
            stdout_log: artifacts.stdout_log.clone(),
            stderr_log: artifacts.stderr_log.clone(),
            notes: vec!["ACP execution completed.".to_string()],
            session_id,
        })
    }

    async fn plan(
        &self,
        context: &WorkerContext,
        artifacts: &StageArtifactSet,
        request: &PlanningRequest,
    ) -> Result<WorkerResult> {
        self.run_exec_stage(
            context,
            None,
            artifacts,
            &context.planner_prompt,
            &context.planner_schema,
            request,
        )
        .await
    }

    async fn build(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &loopsmith_core::domain::FeatureContract,
    ) -> Result<WorkerResult> {
        self.run_exec_stage(
            context,
            Some(feature),
            artifacts,
            &context.builder_prompt,
            &context.builder_schema,
            contract,
        )
        .await
    }

    async fn evaluate(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        request: &EvaluationRequest,
    ) -> Result<WorkerResult> {
        self.run_exec_stage(
            context,
            Some(feature),
            artifacts,
            &context.evaluator_prompt,
            &context.qa_schema,
            request,
        )
        .await
    }

    async fn repair(
        &self,
        context: &WorkerContext,
        feature: &FeatureLayout,
        artifacts: &StageArtifactSet,
        contract: &loopsmith_core::domain::FeatureContract,
        builder_handoff: &BuilderHandoff,
        qa_report: &QaReport,
        previous_session_id: Option<&str>,
    ) -> Result<WorkerResult> {
        #[derive(Serialize)]
        struct RepairPayload<'a> {
            contract: &'a loopsmith_core::domain::FeatureContract,
            builder_handoff: &'a BuilderHandoff,
            qa_report: &'a QaReport,
        }

        let payload = RepairPayload {
            contract,
            builder_handoff,
            qa_report,
        };

        self.run_repair(
            context,
            feature,
            artifacts,
            &context.builder_prompt,
            &context.builder_schema,
            &payload,
            previous_session_id,
        )
        .await
    }
}

fn extract_json_from_output(raw: &str) -> Option<Vec<u8>> {
    let trimmed = raw.trim();

    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end >= start {
                let candidate = &trimmed[start..=end];
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return Some(candidate.as_bytes().to_vec());
                }
            }
        }
    }

    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            if end >= start {
                let candidate = &trimmed[start..=end];
                if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                    return Some(candidate.as_bytes().to_vec());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::extract_json_from_output;

    #[test]
    fn extract_json_from_clean_output() {
        let output = r#"{"goal":"build it","features":[],"risks":[],"checkpoints":[]}"#;
        let result = extract_json_from_output(output);
        assert!(result.is_some());
        let parsed: serde_json::Value =
            serde_json::from_slice(&result.unwrap()).expect("valid json");
        assert_eq!(parsed.get("goal").unwrap().as_str(), Some("build it"));
    }

    #[test]
    fn extract_json_from_wrapped_output() {
        let output = "Here is the result:\n```json\n{\"ok\":true}\n```\nDone.";
        let result = extract_json_from_output(output);
        assert!(result.is_some());
        let parsed: serde_json::Value =
            serde_json::from_slice(&result.unwrap()).expect("valid json");
        assert_eq!(parsed.get("ok").unwrap().as_bool(), Some(true));
    }

    #[test]
    fn extract_json_returns_none_for_non_json() {
        let output = "This is just plain text with no JSON.";
        let result = extract_json_from_output(output);
        assert!(result.is_none());
    }

    #[test]
    fn extract_json_handles_array() {
        let output = r#"[{"id":"f1","title":"Feature"}]"#;
        let result = extract_json_from_output(output);
        assert!(result.is_some());
    }
}
