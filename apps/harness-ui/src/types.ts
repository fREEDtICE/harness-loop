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
  awaiting_feature_confirmation: boolean;
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
  awaiting_feature_confirmation: boolean;
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
  evidence_path: string;
  profile_path: string;
  inference_path: string;
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
  inference_summary: string | null;
  overview: WorkspaceDiscoveryOverview;
};

export type WorkspaceDiscoveryOverview = {
  source_file_count: number;
  repository_count: number;
  dependency_relationship_count: number;
  layer_count: number;
  api_contract_count: number;
  user_journey_count: number;
  e2e_test_case_count: number;
  auth_surface_count: number;
  coding_convention_count: number;
  build_command_count: number;
  test_command_count: number;
  dev_command_count: number;
  tech_stack: string[];
  key_concepts: string[];
  repositories: string[];
  layering_summary: string | null;
  layering_rules: string[];
  layering_ambiguities: string[];
  api_contracts: string[];
  user_journeys: string[];
  e2e_test_cases: string[];
  auth_surfaces: string[];
  coding_conventions: string[];
  build_commands: string[];
  test_commands: string[];
  dev_commands: string[];
  risks: string[];
  inference_count: number;
  strongest_inferences: string[];
  weakest_inferences: string[];
  average_inference_confidence: number | null;
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

export const PLANNER_CONVERSATION_READINESS = [
  "needs_clarification",
  "ready_to_plan",
  "ready_to_build",
] as const;

export type PlannerConversationReadiness =
  (typeof PLANNER_CONVERSATION_READINESS)[number];

export type PlannerConversationTurn = {
  role: "user" | "planner";
  content: string;
};

export type PlannerConversationDraft = {
  workspace_path: string;
  config_path: string;
  request_draft: string;
  prompt_overrides: PromptOverrides;
  feature_limit: number | null;
  conversation: PlannerConversationTurn[];
};

export type PlannerConversationResponse = {
  reply_markdown: string;
  revised_request: string;
  readiness: PlannerConversationReadiness;
  open_questions: string[];
  suggested_features: string[];
  suggested_feature_limit: number | null;
  confirmation_points: string[];
};

export type EditorState = {
  requestDraft: string;
  plannerPrompt: string;
  builderPrompt: string;
  evaluatorPrompt: string;
};
