export type PromptOverrides = {
  planner: string | null;
  builder: string | null;
  evaluator: string | null;
};

export type PromptSnapshot = {
  planner: string;
  builder: string;
  evaluator: string;
};

export type WorkspaceRecord = {
  workspace_path: string;
  display_name: string;
  last_opened_at: string;
  pinned: boolean;
};

export type ActiveRunStage = {
  stage: string;
  attempt: number;
  feature_index: number | null;
  feature_id: string | null;
  started_at: string;
};

export type WorkspaceRunSummary = {
  run_root: string;
  run_title: string;
  created_at: string;
  updated_at: string;
  lifecycle: string;
  final_status: string | null;
  current_feature_index: number;
  total_features: number;
  active_stage: ActiveRunStage | null;
};

export type RunStageRecord = {
  stage: string;
  attempt: number;
  status: string;
  artifact: string;
  stdout_log: string;
  stderr_log: string;
  session_id: string | null;
};

export type FeatureRunState = {
  index: number;
  feature_id: string;
  title: string;
  feature_root: string;
  contract_file: string;
  builder_handoff_file: string;
  qa_report_file: string;
  status: string;
  phase: string;
  repair_attempts_used: number;
  next_evaluate_attempt: number;
  last_session_id: string | null;
  last_qa_status: string | null;
  stages: RunStageRecord[];
};

export type RunState = {
  run_id: string;
  run_title: string;
  created_at: string;
  updated_at: string;
  run_root: string;
  state_file: string;
  manifest_file: string;
  launch_file: string | null;
  request_file: string;
  plan_file: string;
  runtime_plan_file: string;
  source_workspace: string;
  execution_workspace: string;
  lifecycle: string;
  final_status: string | null;
  current_feature_index: number;
  active_stage: ActiveRunStage | null;
  plan_stage: RunStageRecord | null;
  features: FeatureRunState[];
};

export type PromptBundle = {
  defaults: PromptSnapshot;
  effective: PromptSnapshot;
};

export type WorkspaceDiscoveryStatus = {
  workspace_path: string;
  scan_path: string;
  profile_path: string;
  workspace_fingerprint: string;
  profile_fingerprint: string | null;
  last_scanned_at: string;
  last_refreshed_at: string | null;
  last_refresh_error: string | null;
  used_fallback_profile: boolean;
  current_phase: WorkspaceDiscoveryPhase;
};

export const WORKSPACE_DISCOVERY_PHASES = [
  "idle",
  "scanning",
  "reusing_cached_profile",
  "polishing",
  "using_fallback_profile",
  "ready",
  "failed",
] as const;

export type WorkspaceDiscoveryPhase =
  (typeof WORKSPACE_DISCOVERY_PHASES)[number];

export type WorkspaceDiscoveryPayload = {
  status: WorkspaceDiscoveryStatus;
  profile_summary: string | null;
};

export type WorkspacePayload = {
  record: WorkspaceRecord;
  config_path: string;
  prompts: PromptBundle | null;
  discovery: WorkspaceDiscoveryPayload | null;
  runs: WorkspaceRunSummary[];
  current_run: RunState | null;
  config_error: string | null;
};

export type LaunchDraft = {
  workspace_path: string;
  config_path: string;
  request_draft: string;
  prompt_overrides: PromptOverrides;
  feature_limit: number | null;
};

export type EditorState = {
  requestDraft: string;
  plannerPrompt: string;
  builderPrompt: string;
  evaluatorPrompt: string;
};
