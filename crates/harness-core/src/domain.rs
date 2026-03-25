use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub user_request: String,
    pub source_workspace: PathBuf,
    pub feature_limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanningRequest {
    pub user_request: String,
    pub feature_limit: usize,
    pub service_names: Vec<String>,
    pub verification_commands: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanDocument {
    pub goal: String,
    pub features: Vec<Feature>,
    pub risks: Vec<String>,
    pub checkpoints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feature {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureContract {
    pub feature_id: String,
    pub title: String,
    pub scope_notes: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub verification_commands: Vec<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuilderHandoff {
    pub summary: String,
    pub changed_files: Vec<String>,
    pub verification: Vec<String>,
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationRequest {
    pub contract: FeatureContract,
    pub builder_handoff: BuilderHandoff,
    pub dimensions: Vec<String>,
    pub require_screenshots: bool,
    pub service_names: Vec<String>,
    pub verification_commands: Vec<Vec<String>>,
    pub verification_evidence: VerificationEvidence,
    pub screenshot_evidence: Option<ScreenshotEvidence>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
}

impl VerificationStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationCommandResult {
    pub name: String,
    pub command: Vec<String>,
    pub working_dir: PathBuf,
    pub status: VerificationStatus,
    pub exit_code: Option<i32>,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationEvidence {
    pub attempt: usize,
    pub report_file: PathBuf,
    pub results: Vec<VerificationCommandResult>,
}

impl VerificationEvidence {
    pub fn all_passed(&self) -> bool {
        self.results
            .iter()
            .all(|result| result.status == VerificationStatus::Passed)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScreenshotStatus {
    Captured,
    Failed,
}

impl ScreenshotStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Captured => "captured",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotCommandResult {
    pub name: String,
    pub command: Vec<String>,
    pub working_dir: PathBuf,
    pub output_file: PathBuf,
    pub status: ScreenshotStatus,
    pub exit_code: Option<i32>,
    pub bytes: Option<u64>,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenshotEvidence {
    pub attempt: usize,
    pub report_file: PathBuf,
    pub results: Vec<ScreenshotCommandResult>,
}

impl ScreenshotEvidence {
    pub fn all_captured(&self) -> bool {
        self.results
            .iter()
            .all(|result| result.status == ScreenshotStatus::Captured)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStage {
    Plan,
    Build,
    Evaluate,
    Repair,
}

impl WorkerStage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
            Self::Evaluate => "evaluate",
            Self::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Prepared,
    Executed,
}

impl WorkerStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Executed => "executed",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResult {
    pub stage: WorkerStage,
    pub status: WorkerStatus,
    pub command: Vec<String>,
    pub prompt_file: PathBuf,
    pub output_file: PathBuf,
    pub stdout_log: PathBuf,
    pub stderr_log: PathBuf,
    pub notes: Vec<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaCheck {
    pub name: String,
    pub command: Vec<String>,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QaStatus {
    Pass,
    Fail,
    Inconclusive,
}

impl QaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Inconclusive => "inconclusive",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QaReport {
    pub status: QaStatus,
    pub summary: String,
    pub findings: Vec<String>,
    pub next_actions: Vec<String>,
    pub checks: Vec<QaCheck>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunLifecycleStatus {
    Planning,
    Running,
    Passed,
    Failed,
}

impl RunLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeatureLifecycleStatus {
    Pending,
    Running,
    Passed,
    Failed,
}

impl FeatureLifecycleStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FeaturePhase {
    PendingBuild,
    PendingEvaluate,
    PendingRepair,
    Complete,
}

impl FeaturePhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingBuild => "pending_build",
            Self::PendingEvaluate => "pending_evaluate",
            Self::PendingRepair => "pending_repair",
            Self::Complete => "complete",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStageRecord {
    pub stage: WorkerStage,
    pub attempt: usize,
    pub status: WorkerStatus,
    pub artifact: PathBuf,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureRunState {
    pub index: usize,
    pub feature_id: String,
    pub title: String,
    pub feature_root: PathBuf,
    pub contract_file: PathBuf,
    pub builder_handoff_file: PathBuf,
    pub qa_report_file: PathBuf,
    pub status: FeatureLifecycleStatus,
    pub phase: FeaturePhase,
    pub repair_attempts_used: usize,
    pub next_evaluate_attempt: usize,
    pub last_session_id: Option<String>,
    pub last_qa_status: Option<QaStatus>,
    pub stages: Vec<RunStageRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_id: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub run_root: PathBuf,
    pub state_file: PathBuf,
    pub manifest_file: PathBuf,
    pub request_file: PathBuf,
    pub plan_file: PathBuf,
    pub runtime_plan_file: PathBuf,
    pub source_workspace: PathBuf,
    pub execution_workspace: PathBuf,
    pub lifecycle: RunLifecycleStatus,
    pub final_status: Option<QaStatus>,
    pub current_feature_index: usize,
    pub plan_stage: Option<RunStageRecord>,
    pub features: Vec<FeatureRunState>,
}

impl PlanningRequest {
    pub fn synthesize_plan(&self) -> PlanDocument {
        let normalized_request = normalized_request(&self.user_request);
        let goal = normalized_request
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Implement the requested change")
            .trim()
            .to_string();
        let feature_limit = self.feature_limit.max(1);
        let (slice_descriptions, explicit_slice_count) =
            self.derive_feature_slices(&goal, feature_limit);

        let features = slice_descriptions
            .iter()
            .enumerate()
            .map(|(index, slice)| Feature {
                id: format!("feature-{:03}", index + 1),
                title: trim_title(slice),
                summary: if index == 0 && slice == &goal {
                    normalized_request.clone()
                } else {
                    format!(
                        "Bounded implementation slice {} derived from the request: {}.",
                        index + 1,
                        slice
                    )
                },
                acceptance_criteria: self.acceptance_criteria_for(slice),
            })
            .collect();

        PlanDocument {
            goal,
            features,
            risks: self.risks_for(feature_limit, explicit_slice_count),
            checkpoints: self.checkpoints(),
        }
    }

    fn derive_feature_slices(&self, goal: &str, feature_limit: usize) -> (Vec<String>, usize) {
        let lines = self
            .user_request
            .lines()
            .map(clean_request_line)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>();
        let mut slices = Vec::new();

        if lines.len() > 1 {
            let has_structured_follow_ups = self
                .user_request
                .lines()
                .skip(1)
                .any(|line| is_structured_request_line(line.trim_start()));

            if has_structured_follow_ups {
                slices.extend(lines.iter().skip(1).cloned());
            } else {
                slices.extend(lines.iter().cloned());
            }
        } else if let Some(line) = lines.first() {
            let sentence_slices = split_sentence_slices(line);
            if sentence_slices.len() > 1 {
                slices.extend(sentence_slices);
            }
        }

        if slices.is_empty() {
            slices.push(goal.to_string());
        }

        let explicit_slice_count = slices.len().min(feature_limit);

        while slices.len() < feature_limit {
            slices.push(self.follow_up_slice(slices.len() + 1, goal));
        }

        slices.truncate(feature_limit);
        (slices, explicit_slice_count)
    }

    fn follow_up_slice(&self, sequence: usize, goal: &str) -> String {
        if sequence == 2 && !self.service_names.is_empty() {
            format!("Stabilize runtime readiness for {goal}")
        } else if sequence == 2 && !self.verification_commands.is_empty() {
            format!("Close deterministic verification gaps for {goal}")
        } else {
            format!("Complete follow-up slice {sequence} for {goal}")
        }
    }

    fn acceptance_criteria_for(&self, slice: &str) -> Vec<String> {
        let mut criteria = vec![format!(
            "Deliver the concrete outcome for this slice: {slice}."
        )];

        if self.verification_commands.is_empty() {
            criteria.push(
                "Leave deterministic evidence on disk or in logs that an evaluator can inspect."
                    .to_string(),
            );
        } else if has_placeholder_verification(&self.verification_commands) {
            criteria.push(
                "Replace placeholder verification with a real proof point before calling this slice complete."
                    .to_string(),
            );
        } else {
            criteria.push(
                "Configured verification commands can validate this slice without manual interpretation."
                    .to_string(),
            );
        }

        if self.service_names.is_empty() {
            criteria.push(
                "Persist artifacts that support inspect, resume, and evaluator review.".to_string(),
            );
        } else {
            criteria.push(format!(
                "Configured runtime services stay ready after this slice: {}.",
                self.service_names.join(", ")
            ));
        }

        criteria
    }

    fn risks_for(&self, feature_limit: usize, explicit_slice_count: usize) -> Vec<String> {
        let mut risks = Vec::new();

        if self.user_request.trim().is_empty() {
            risks.push(
                "The request is empty, so the plan must infer scope from repository evidence instead of explicit user intent."
                    .to_string(),
            );
        }

        if self.verification_commands.is_empty() {
            risks.push(
                "No deterministic verification commands are configured, so completion may depend on manual judgment."
                    .to_string(),
            );
        } else if has_placeholder_verification(&self.verification_commands) {
            risks.push(
                "Configured verification is placeholder-only, so passing runs may not prove real user-visible behavior."
                    .to_string(),
            );
        }

        if !self.service_names.is_empty() {
            risks.push(format!(
                "The run depends on runtime readiness for configured services: {}.",
                self.service_names.join(", ")
            ));
        }

        if explicit_slice_count < feature_limit {
            risks.push(
                "The request does not spell out enough independent slices for the requested feature limit, so later features are planner-derived follow-ups."
                    .to_string(),
            );
        }

        risks
    }

    fn checkpoints(&self) -> Vec<String> {
        let mut checkpoints = vec![
            "Review the first slice and its contract before starting a long build or repair loop."
                .to_string(),
        ];

        if self.verification_commands.is_empty() {
            checkpoints.push(
                "Define at least one deterministic verification command before expanding scope beyond the first slice."
                    .to_string(),
            );
        } else {
            checkpoints.push(
                "Run the configured verification commands and inspect the persisted logs after each slice before continuing."
                    .to_string(),
            );
        }

        if self.service_names.is_empty() {
            checkpoints.push(
                "Persist request, plan, contract, builder handoff, and QA artifacts as reset points between slices."
                    .to_string(),
            );
        } else {
            checkpoints.push(format!(
                "Confirm configured runtime services are ready before accepting user-visible behavior: {}.",
                self.service_names.join(", ")
            ));
        }

        checkpoints
    }
}

impl FeatureContract {
    pub fn from_feature(feature: &Feature, verification_commands: &[Vec<String>]) -> Self {
        Self {
            feature_id: feature.id.clone(),
            title: feature.title.clone(),
            scope_notes: vec![
                "Keep the control loop outside the worker process.".to_string(),
                "Treat artifacts on disk as the durable source of truth.".to_string(),
                "Favor restartable sessions over excessively long, fragile context windows."
                    .to_string(),
            ],
            acceptance_criteria: feature.acceptance_criteria.clone(),
            verification_commands: verification_commands.to_vec(),
        }
    }
}

fn trim_title(goal: &str) -> String {
    const MAX_LEN: usize = 64;

    if goal.chars().count() <= MAX_LEN {
        return goal.to_string();
    }

    let mut trimmed = goal.chars().take(MAX_LEN - 3).collect::<String>();
    trimmed.push_str("...");
    trimmed
}

fn normalized_request(user_request: &str) -> String {
    let normalized = user_request.trim();
    if normalized.is_empty() {
        "Implement the requested change".to_string()
    } else {
        normalized.to_string()
    }
}

fn clean_request_line(line: &str) -> String {
    let trimmed = line.trim();
    let stripped = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed);

    strip_numbered_prefix(stripped).trim().to_string()
}

fn strip_numbered_prefix(line: &str) -> &str {
    let digit_prefix_len = line
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .map(char::len_utf8)
        .sum::<usize>();

    if digit_prefix_len == 0 {
        return line;
    }

    let remainder = &line[digit_prefix_len..];
    if let Some(stripped) = remainder.strip_prefix(". ") {
        stripped
    } else {
        line
    }
}

fn is_structured_request_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
        return true;
    }

    let digit_prefix_len = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .map(char::len_utf8)
        .sum::<usize>();

    digit_prefix_len > 0 && trimmed[digit_prefix_len..].starts_with(". ")
}

fn split_sentence_slices(line: &str) -> Vec<String> {
    let mut slices = Vec::new();
    let mut current = String::new();

    for ch in line.chars() {
        current.push(ch);
        if matches!(ch, '.' | ';') {
            let trimmed = current.trim();
            if !trimmed.is_empty() {
                slices.push(trimmed.to_string());
            }
            current.clear();
        }
    }

    let trailing = current.trim();
    if !trailing.is_empty() {
        slices.push(trailing.to_string());
    }

    slices
}

fn has_placeholder_verification(commands: &[Vec<String>]) -> bool {
    !commands.is_empty()
        && commands
            .iter()
            .all(|command| matches_placeholder_command(command))
}

fn matches_placeholder_command(command: &[String]) -> bool {
    matches!(command, [single] if single == "true")
        || matches!(command, [env, truthy] if env == "/usr/bin/env" && truthy == "true")
        || matches!(
            command,
            [shell, flag, script]
                if (shell == "/bin/sh" || shell == "sh")
                    && flag == "-c"
                    && script.trim() == "true"
        )
}

#[cfg(test)]
mod tests {
    use super::PlanningRequest;

    #[test]
    fn synthesized_plan_keeps_requested_feature_count() {
        let plan = PlanningRequest {
            user_request: "Build a harness".to_string(),
            feature_limit: 2,
            service_names: Vec::new(),
            verification_commands: Vec::new(),
        }
        .synthesize_plan();
        assert_eq!(plan.features.len(), 2);
        assert_eq!(plan.features[0].id, "feature-001");
        assert_eq!(plan.features[1].id, "feature-002");
    }

    #[test]
    fn synthesized_plan_uses_structured_follow_up_lines_as_slices() {
        let plan = PlanningRequest {
            user_request: "Ship the auth flow\n- Add signup form\n- Add email verification\n"
                .to_string(),
            feature_limit: 2,
            service_names: vec!["web".to_string()],
            verification_commands: vec![vec!["cargo".to_string(), "test".to_string()]],
        }
        .synthesize_plan();

        assert_eq!(plan.goal, "Ship the auth flow");
        assert_eq!(plan.features[0].title, "Add signup form");
        assert_eq!(plan.features[1].title, "Add email verification");
    }

    #[test]
    fn synthesized_plan_flags_placeholder_verification() {
        let plan = PlanningRequest {
            user_request: "Build the settings page".to_string(),
            feature_limit: 1,
            service_names: Vec::new(),
            verification_commands: vec![vec!["/usr/bin/env".to_string(), "true".to_string()]],
        }
        .synthesize_plan();

        assert!(
            plan.risks
                .iter()
                .any(|risk| risk.contains("placeholder-only"))
        );
        assert!(
            plan.features[0]
                .acceptance_criteria
                .iter()
                .any(|criterion| criterion.contains("Replace placeholder verification"))
        );
    }
}
