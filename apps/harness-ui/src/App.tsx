import { invoke } from "@tauri-apps/api/core";
import {
  startTransition,
  useEffect,
  useMemo,
  useRef,
  useState,
  useTransition,
} from "react";
import UIStateReporter from "./UIStateReporter";

type PromptOverrides = {
  planner: string | null;
  builder: string | null;
  evaluator: string | null;
};

type PromptSnapshot = {
  planner: string;
  builder: string;
  evaluator: string;
};

type WorkspaceProfile = {
  workspace_path: string;
  display_name: string;
  preferred_config_path: string | null;
  request_draft: string;
  prompt_overrides: PromptOverrides;
  last_opened_at: string;
  last_run_root: string | null;
  pinned: boolean;
};

type ActiveRunStage = {
  stage: string;
  attempt: number;
  feature_index: number | null;
  feature_id: string | null;
  started_at: string;
};

type WorkspaceRunSummary = {
  run_root: string;
  created_at: string;
  updated_at: string;
  lifecycle: string;
  final_status: string | null;
  current_feature_index: number;
  total_features: number;
  active_stage: ActiveRunStage | null;
};

type RunStageRecord = {
  stage: string;
  attempt: number;
  status: string;
  artifact: string;
  session_id: string | null;
};

type FeatureRunState = {
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

type RunState = {
  run_id: string;
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

type PromptBundle = {
  defaults: PromptSnapshot;
  effective: PromptSnapshot;
};

type WorkspacePayload = {
  profile: WorkspaceProfile;
  prompts: PromptBundle | null;
  runs: WorkspaceRunSummary[];
  current_run: RunState | null;
  config_error: string | null;
};

type LaunchDraft = {
  workspace_path: string;
  config_path: string;
  request_draft: string;
  prompt_overrides: PromptOverrides;
  feature_limit: number | null;
};

type EditorState = {
  configPath: string;
  requestDraft: string;
  plannerPrompt: string;
  builderPrompt: string;
  evaluatorPrompt: string;
};

const emptyOverrides: PromptOverrides = {
  planner: null,
  builder: null,
  evaluator: null,
};

export default function App() {
  const [profiles, setProfiles] = useState<WorkspaceProfile[]>([]);
  const [selectedWorkspacePath, setSelectedWorkspacePath] = useState<string | null>(null);
  const [workspace, setWorkspace] = useState<WorkspacePayload | null>(null);
  const [editors, setEditors] = useState<EditorState | null>(null);
  const [statusMessage, setStatusMessage] = useState("Open a workspace to begin.");
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [isPending, startUiTransition] = useTransition();
  const pollTimerRef = useRef<number | null>(null);
  const saveTimerRef = useRef<number | null>(null);

  useEffect(() => {
    void refreshProfiles();
  }, []);

  const selectedProfile = useMemo(() => workspace?.profile ?? null, [workspace]);
  const promptDefaults = workspace?.prompts?.defaults ?? null;
  const activeRun = workspace?.current_run ?? null;
  const activeRunRoot = activeRun?.run_root ?? workspace?.runs.find((run) => run.lifecycle === "running")?.run_root ?? null;

  useEffect(() => {
    if (!selectedWorkspacePath || !selectedProfile || !editors) {
      return;
    }

    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
    }

    const updatedProfile = buildUpdatedProfile(selectedProfile, editors, promptDefaults);
    saveTimerRef.current = window.setTimeout(() => {
      void saveProfile(updatedProfile);
    }, 350);

    return () => {
      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
        saveTimerRef.current = null;
      }
    };
  }, [selectedWorkspacePath, selectedProfile, editors, promptDefaults]);

  useEffect(() => {
    if (!selectedWorkspacePath || !workspace?.profile.preferred_config_path) {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
      return;
    }

    if (!activeRunRoot && !isRunning) {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
      return;
    }

    pollTimerRef.current = window.setInterval(() => {
      void refreshWorkspace(selectedWorkspacePath, false);
    }, 1250);

    return () => {
      if (pollTimerRef.current !== null) {
        window.clearInterval(pollTimerRef.current);
        pollTimerRef.current = null;
      }
    };
  }, [selectedWorkspacePath, workspace?.profile.preferred_config_path, activeRunRoot, isRunning]);

  async function refreshProfiles() {
    try {
      const nextProfiles = await invoke<WorkspaceProfile[]>("load_profiles");
      setProfiles(nextProfiles);
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function openWorkspace(path?: string) {
    const requestedPath = path?.trim();
    const workspacePath =
      (requestedPath ? requestedPath : null) ??
      (await invoke<string | null>("pick_workspace_folder").catch((error) => {
        setErrorMessage(readError(error));
        return null;
      }));

    if (!workspacePath) {
      return;
    }

    startUiTransition(() => {
      setSelectedWorkspacePath(workspacePath);
    });
    await refreshWorkspace(workspacePath, true);
  }

  async function refreshWorkspace(workspacePath: string, announce = false) {
    try {
      const payload = await invoke<WorkspacePayload>("load_workspace", {
        workspacePath: workspacePath,
      });
      startTransition(() => {
        setWorkspace(payload);
        setEditors({
          configPath: payload.profile.preferred_config_path ?? "",
          requestDraft: payload.profile.request_draft,
          plannerPrompt:
            payload.prompts?.effective.planner ??
            payload.profile.prompt_overrides.planner ??
            "",
          builderPrompt:
            payload.prompts?.effective.builder ??
            payload.profile.prompt_overrides.builder ??
            "",
          evaluatorPrompt:
            payload.prompts?.effective.evaluator ??
            payload.profile.prompt_overrides.evaluator ??
            "",
        });
        setProfiles((current) => upsertProfile(current, payload.profile));
      });
      if (announce) {
        setStatusMessage(`Opened ${payload.profile.display_name}.`);
      }
      setErrorMessage(payload.config_error);
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function saveProfile(profile: WorkspaceProfile) {
    try {
      const nextProfiles = await invoke<WorkspaceProfile[]>("save_profile", { profile });
      setProfiles(nextProfiles);
      setWorkspace((current) =>
        current
          ? {
              ...current,
              profile,
            }
          : current,
      );
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function removeProfile(workspacePath: string) {
    try {
      const nextProfiles = await invoke<WorkspaceProfile[]>("remove_profile", {
        workspacePath: workspacePath,
      });
      setProfiles(nextProfiles);
      if (selectedWorkspacePath === workspacePath) {
        setSelectedWorkspacePath(null);
        setWorkspace(null);
        setEditors(null);
      }
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function chooseConfigFile() {
    const configPath = await invoke<string | null>("pick_config_file").catch((error) => {
      setErrorMessage(readError(error));
      return null;
    });
    if (!configPath || !editors) {
      return;
    }
    setEditors({
      ...editors,
      configPath,
    });
    await reloadPrompts(configPath, currentOverrides(editors, promptDefaults));
  }

  async function reloadPrompts(configPath?: string, overrides?: PromptOverrides) {
    const path = configPath ?? editors?.configPath ?? "";
    if (!path.trim()) {
      setErrorMessage("Select a config file first.");
      return;
    }

    try {
      const bundle = await invoke<PromptBundle>("load_prompt_bundle", {
        configPath: path,
        overrides: overrides ?? currentOverrides(editors, promptDefaults),
      });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              prompts: bundle,
              config_error: null,
            }
          : current,
      );
      setEditors((current) =>
        current
          ? {
              ...current,
              plannerPrompt: bundle.effective.planner,
              builderPrompt: bundle.effective.builder,
              evaluatorPrompt: bundle.effective.evaluator,
            }
          : current,
      );
      setErrorMessage(null);
      setStatusMessage("Loaded prompts from config.");
      if (selectedWorkspacePath) {
        await refreshWorkspace(selectedWorkspacePath, false);
      }
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function resetPromptOverrides() {
    if (!editors) {
      return;
    }
    await reloadPrompts(editors.configPath, emptyOverrides);
  }

  async function inspectRun(runRoot: string) {
    const configPath = editors?.configPath ?? workspace?.profile.preferred_config_path ?? "";
    if (!configPath) {
      setErrorMessage("Select a config file before inspecting a run.");
      return;
    }

    try {
      const run = await invoke<RunState>("inspect_run", {
        configPath: configPath,
        runRoot: runRoot,
      });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              current_run: run,
            }
          : current,
      );
    } catch (error) {
      setErrorMessage(readError(error));
    }
  }

  async function launchRun() {
    if (!workspace || !editors) {
      return;
    }

    if (!editors.configPath.trim()) {
      setErrorMessage("Select a config file first.");
      return;
    }

    setIsRunning(true);
    setStatusMessage("Starting harness run.");
    setErrorMessage(null);

    const draft: LaunchDraft = {
      workspace_path: workspace.profile.workspace_path,
      config_path: editors.configPath,
      request_draft: editors.requestDraft,
      prompt_overrides: currentOverrides(editors, promptDefaults),
      feature_limit: null,
    };

    try {
      const run = await invoke<RunState>("start_run", { draft });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              current_run: run,
            }
          : current,
      );
      setStatusMessage("Harness run completed.");
      if (selectedWorkspacePath) {
        await refreshWorkspace(selectedWorkspacePath, false);
      }
    } catch (error) {
      setErrorMessage(readError(error));
      setStatusMessage("Harness run failed.");
    } finally {
      setIsRunning(false);
    }
  }

  async function resumeRun(runRoot: string) {
    const configPath = editors?.configPath ?? workspace?.profile.preferred_config_path ?? "";
    if (!configPath) {
      setErrorMessage("Select a config file before resuming a run.");
      return;
    }

    setIsRunning(true);
    setStatusMessage(`Resuming ${basename(runRoot)}.`);
    setErrorMessage(null);

    try {
      const run = await invoke<RunState>("resume_run", {
        configPath: configPath,
        runRoot: runRoot,
      });
      setWorkspace((current) =>
        current
          ? {
              ...current,
              current_run: run,
            }
          : current,
      );
      setStatusMessage("Harness resume completed.");
      if (selectedWorkspacePath) {
        await refreshWorkspace(selectedWorkspacePath, false);
      }
    } catch (error) {
      setErrorMessage(readError(error));
      setStatusMessage("Harness resume failed.");
    } finally {
      setIsRunning(false);
    }
  }

  const runsDeferred = useMemo(() => workspace?.runs ?? [], [workspace?.runs]);

  return (
    <div className="app-shell" data-testid="app-shell">
      <div className="backdrop backdrop-a" />
      <div className="backdrop backdrop-b" />
      <UIStateReporter
        profiles={profiles}
        selectedWorkspacePath={selectedWorkspacePath}
        workspace={workspace}
        editors={editors}
        statusMessage={statusMessage}
        errorMessage={errorMessage}
        isRunning={isRunning}
        isPending={isPending}
      />
      <header className="hero" data-testid="hero">
        <div>
          <p className="eyebrow">Harness Control Room</p>
          <h1>Codex Harness UI</h1>
          <p className="hero-copy">
            Workspace launcher, prompt studio, and durable loop monitor for the
            Rust harness.
          </p>
        </div>
        <div className="hero-status" data-testid="hero-status">
          <div className="status-chip" data-testid="status-message">{statusMessage}</div>
          {errorMessage ? <div className="status-chip error" data-testid="error-message">{errorMessage}</div> : null}
        </div>
      </header>

      <main className="layout">
        <aside className="panel sidebar" data-testid="sidebar">
          <button
            className="sidebar-new-button"
            data-testid="sidebar-open-workspace"
            onClick={() => void openWorkspace()}
          >
            <span className="sidebar-new-icon">+</span>
            New Workspace
          </button>

          <div className="sidebar-section-label" data-testid="sidebar-workspace-count">
            Workspaces · {profiles.length}
          </div>

          <div className="workspace-list" data-testid="sidebar-workspace-list">
            {profiles.length === 0 ? (
              <div className="empty-state" data-testid="sidebar-empty-state">
                Open a folder to get started.
              </div>
            ) : (
              profiles.map((profile) => {
                const selected = selectedWorkspacePath === profile.workspace_path;
                const running = selected && isRunning;
                return (
                  <button
                    key={profile.workspace_path}
                    data-testid={`sidebar-workspace-${basename(profile.workspace_path)}`}
                    className={`sidebar-item ${selected ? "selected" : ""}`}
                    onClick={() => void openWorkspace(profile.workspace_path)}
                  >
                    <span className={`sidebar-item-icon ${running ? "running" : profile.last_run_root ? "done" : "idle"}`}>
                      {running ? "◉" : profile.last_run_root ? "✓" : "○"}
                    </span>
                    <div className="sidebar-item-text">
                      <span className="sidebar-item-name">{basename(profile.workspace_path)}</span>
                      <span className="sidebar-item-desc">
                        {running
                          ? "Running…"
                          : profile.last_run_root
                            ? `Last run · ${basename(profile.last_run_root)}`
                            : "No runs yet"}
                      </span>
                    </div>
                    <span
                      className="sidebar-item-remove"
                      data-testid={`sidebar-remove-${basename(profile.workspace_path)}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        void removeProfile(profile.workspace_path);
                      }}
                    >
                      ×
                    </span>
                  </button>
                );
              })
            )}
          </div>
        </aside>

        {workspace && editors ? (
          <section className="content-grid">
            <section className="panel launch-panel" data-testid="launch-panel">
              <div className="panel-header">
                <div>
                  <p className="panel-kicker">Launch Brief</p>
                  <h2 data-testid="launch-title">
                    {workspace.profile.display_name}
                  </h2>
                </div>
                {isPending ? <div className="subtle-pill" data-testid="launch-loading">Loading</div> : null}
              </div>

              <div className="field-group">
                <label>Workspace path</label>
                <code data-testid="launch-workspace-path">{workspace.profile.workspace_path}</code>
              </div>
              <div className="field-group">
                <label>Config file</label>
                <div className="split-input">
                  <input
                    data-testid="launch-config-input"
                    value={editors.configPath}
                    onChange={(event) =>
                      setEditors({ ...editors, configPath: event.target.value })
                    }
                    placeholder="config/codex-cli.toml"
                  />
                  <button data-testid="launch-config-choose" onClick={() => void chooseConfigFile()}>Choose</button>
                  <button
                    className="secondary-button"
                    data-testid="launch-config-reload"
                    onClick={() => void reloadPrompts()}
                  >
                    Reload
                  </button>
                </div>
                {workspace.config_error ? (
                  <p className="field-error" data-testid="launch-config-error">{workspace.config_error}</p>
                ) : null}
              </div>
              <div className="field-group">
                <label>Request</label>
                <textarea
                  data-testid="launch-request-textarea"
                  value={editors.requestDraft}
                  onChange={(event) =>
                    setEditors({ ...editors, requestDraft: event.target.value })
                  }
                  rows={9}
                  placeholder="Describe the feature slice to build."
                />
              </div>
              <div className="action-row">
                <button
                  className="primary-button"
                  data-testid="launch-start-button"
                  onClick={() => void launchRun()}
                  disabled={isRunning}
                >
                  {isRunning ? "Running…" : "Start Run"}
                </button>
                <button
                  className="secondary-button"
                  data-testid="launch-refresh-button"
                  onClick={() => selectedWorkspacePath && void refreshWorkspace(selectedWorkspacePath, false)}
                >
                  Refresh Workspace
                </button>
              </div>
            </section>

          <section className="panel prompt-panel" data-testid="prompt-panel">
              <div className="panel-header">
                <div>
                  <p className="panel-kicker">Prompt Studio</p>
                  <h2>Planner, Builder, Evaluator</h2>
                </div>
                <button className="secondary-button" data-testid="prompt-reset-overrides" onClick={() => void resetPromptOverrides()}>
                  Reset Overrides
                </button>
              </div>

              <div className="prompt-grid" data-testid="prompt-grid">
                <PromptEditor
                  testid="prompt-editor-planner"
                  title="Planner"
                  value={editors.plannerPrompt}
                  onChange={(value) => setEditors({ ...editors, plannerPrompt: value })}
                />
                <PromptEditor
                  testid="prompt-editor-builder"
                  title="Builder"
                  value={editors.builderPrompt}
                  onChange={(value) => setEditors({ ...editors, builderPrompt: value })}
                />
                <PromptEditor
                  testid="prompt-editor-evaluator"
                  title="Evaluator"
                  value={editors.evaluatorPrompt}
                  onChange={(value) => setEditors({ ...editors, evaluatorPrompt: value })}
                />
              </div>
            </section>

          <section className="panel history-panel" data-testid="history-panel">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Run Ledger</p>
                <h2>Workspace History</h2>
              </div>
            </div>

            {runsDeferred.length === 0 ? (
              <div className="empty-state" data-testid="history-empty-state">No runs discovered for this workspace yet.</div>
            ) : (
              <div className="run-list" data-testid="history-run-list">
                {runsDeferred.map((run) => (
                  <article key={run.run_root} className="run-card" data-testid={`history-run-${basename(run.run_root)}`}>
                    <div className="run-card-top">
                      <strong>{basename(run.run_root)}</strong>
                      <span className={`state-pill state-${run.lifecycle}`}>
                        {run.final_status
                          ? `${run.lifecycle} / ${run.final_status}`
                          : run.lifecycle}
                      </span>
                    </div>
                    <p className="workspace-meta">{formatDate(run.updated_at)}</p>
                    <p className="workspace-meta">
                      Feature {run.current_feature_index} of {run.total_features}
                    </p>
                    {run.active_stage ? (
                      <p className="workspace-meta accent">
                        Active {run.active_stage.stage} attempt {run.active_stage.attempt}
                      </p>
                    ) : null}
                    <div className="action-row compact">
                      <button
                        className="secondary-button"
                        data-testid={`history-inspect-${basename(run.run_root)}`}
                        onClick={() => void inspectRun(run.run_root)}
                      >
                        Inspect
                      </button>
                      {run.lifecycle === "running" ? (
                        <button
                          className="primary-button"
                          data-testid={`history-resume-${basename(run.run_root)}`}
                          onClick={() => void resumeRun(run.run_root)}
                          disabled={isRunning}
                        >
                          Resume
                        </button>
                      ) : null}
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>

          <section className="panel monitor-panel" data-testid="monitor-panel">
            <div className="panel-header">
              <div>
                <p className="panel-kicker">Loop Monitor</p>
                <h2>Current Run</h2>
              </div>
              {activeRun ? (
                <span className={`state-pill state-${activeRun.lifecycle}`} data-testid="monitor-lifecycle-pill">
                  {activeRun.final_status
                    ? `${activeRun.lifecycle} / ${activeRun.final_status}`
                    : activeRun.lifecycle}
                </span>
              ) : null}
            </div>

            {activeRun ? (
              <>
                <div className="monitor-summary" data-testid="monitor-summary">
                  <StatCard
                    testid="monitor-stat-run-root"
                    label="Run Root"
                    value={basename(activeRun.run_root)}
                    detail={activeRun.run_root}
                  />
                  <StatCard
                    testid="monitor-stat-progress"
                    label="Progress"
                    value={`${activeRun.current_feature_index}/${activeRun.features.length}`}
                    detail="current feature / total features"
                  />
                  <StatCard
                    testid="monitor-stat-active-stage"
                    label="Active Stage"
                    value={
                      activeRun.active_stage
                        ? `${activeRun.active_stage.stage} ${activeRun.active_stage.attempt}`
                        : "idle"
                    }
                    detail={
                      activeRun.active_stage?.feature_id
                        ? `feature ${activeRun.active_stage.feature_id}`
                        : "no active stage"
                    }
                  />
                </div>
                <div className="timeline" data-testid="monitor-timeline">
                  {activeRun.plan_stage ? (
                    <StageRow
                      testid="monitor-plan-stage"
                      heading="Plan"
                      text={`attempt ${activeRun.plan_stage.attempt} / ${activeRun.plan_stage.status}`}
                    />
                  ) : null}
                  {activeRun.features.map((feature) => (
                    <div key={feature.feature_id} className="feature-block" data-testid={`monitor-feature-${feature.feature_id}`}>
                      <div className="feature-block-header">
                        <div>
                          <h3>{feature.title}</h3>
                          <p>
                            {feature.feature_id} · {feature.status} · {feature.phase}
                          </p>
                        </div>
                        <span className="subtle-pill">
                          repairs {feature.repair_attempts_used}
                        </span>
                      </div>
                      <div className="stage-list">
                        {feature.stages.map((stage) => (
                          <StageRow
                            key={`${feature.feature_id}-${stage.stage}-${stage.attempt}`}
                            testid={`monitor-stage-${feature.feature_id}-${stage.stage}-${stage.attempt}`}
                            heading={stage.stage}
                            text={`attempt ${stage.attempt} / ${stage.status}`}
                          />
                        ))}
                      </div>
                    </div>
                  ))}
                </div>
              </>
            ) : (
              <div className="empty-state" data-testid="monitor-empty-state">Select a run from the ledger to inspect it here.</div>
            )}
          </section>
          </section>
        ) : (
          <HarnessLanding />
        )}
      </main>
    </div>
  );
}

function HarnessLanding() {
  return (
    <section className="landing" data-testid="landing">
      <div className="landing-hero">
        <h2>The Entropy Problem</h2>
        <p className="landing-subtitle">
          Why AI-generated code drifts — and how this harness fights back.
        </p>
      </div>

      <div className="landing-columns">
        <div className="landing-card">
          <div className="landing-card-header">
            <div className="landing-card-icon entropy">⚠</div>
            <h3>AI Coding Increases Entropy</h3>
          </div>
          <ul className="landing-list">
            <li>The model sees only a slice of the system at a time</li>
            <li>It optimizes locally, missing cross-file constraints</li>
            <li>Small inconsistencies accumulate across edits</li>
            <li>Later changes build on already-drifted assumptions</li>
            <li>Without explicit contracts, the system degrades silently</li>
          </ul>
        </div>

        <div className="landing-card">
          <div className="landing-card-header">
            <div className="landing-card-icon harness">⟳</div>
            <h3>The Harness Loop</h3>
          </div>
          <p className="landing-card-desc">
            A durable, checkpoint-resumable cycle that continuously reduces entropy
            through deterministic verification and targeted repair.
          </p>
        </div>
      </div>

      <div className="landing-diagram" data-testid="landing-loop-diagram">
        <svg viewBox="0 0 780 320" fill="none" xmlns="http://www.w3.org/2000/svg">
          <defs>
            <marker id="arrow" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#484f58" />
            </marker>
            <marker id="arrow-green" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#3fb950" />
            </marker>
            <marker id="arrow-red" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#f85149" />
            </marker>
            <marker id="arrow-blue" markerWidth="8" markerHeight="6" refX="7" refY="3" orient="auto">
              <path d="M0 0 L8 3 L0 6" fill="#58a6ff" />
            </marker>
          </defs>

          {/* User request input */}
          <rect x="40" y="24" width="120" height="50" rx="10" fill="#161b22" stroke="#484f58" strokeWidth="1.5" />
          <text x="100" y="46" textAnchor="middle" fill="#c9d1d9" fontSize="11" fontWeight="700">USER REQUEST</text>
          <text x="100" y="62" textAnchor="middle" fill="#484f58" fontSize="10">Feature goal</text>
          <line x1="100" y1="74" x2="100" y2="115" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          {/* Plan box */}
          <rect x="40" y="120" width="120" height="60" rx="10" fill="#161b22" stroke="#d29922" strokeWidth="1.5" />
          <text x="100" y="145" textAnchor="middle" fill="#d29922" fontSize="11" fontWeight="700">PLAN</text>
          <text x="100" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">Split features</text>

          {/* Arrow: Plan → Build */}
          <line x1="160" y1="150" x2="215" y2="150" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          {/* Build box */}
          <rect x="220" y="120" width="120" height="60" rx="10" fill="#161b22" stroke="#58a6ff" strokeWidth="1.5" />
          <text x="280" y="145" textAnchor="middle" fill="#58a6ff" fontSize="11" fontWeight="700">BUILD</text>
          <text x="280" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">Generate code</text>

          {/* Arrow: Build → Evaluate */}
          <line x1="340" y1="150" x2="395" y2="150" stroke="#484f58" strokeWidth="1.5" markerEnd="url(#arrow)" />

          {/* Evaluate box */}
          <rect x="400" y="120" width="120" height="60" rx="10" fill="#161b22" stroke="#8b5cf6" strokeWidth="1.5" />
          <text x="460" y="145" textAnchor="middle" fill="#8b5cf6" fontSize="11" fontWeight="700">EVALUATE</text>
          <text x="460" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">Verify + QA</text>

          {/* Arrow: Evaluate → Pass (straight right) */}
          <line x1="520" y1="150" x2="615" y2="150" stroke="#3fb950" strokeWidth="1.5" markerEnd="url(#arrow-green)" />
          <text x="568" y="142" textAnchor="middle" fill="#3fb950" fontSize="10" fontWeight="600">Pass</text>

          {/* Next Feature box (same row as Evaluate) */}
          <rect x="620" y="120" width="140" height="60" rx="10" fill="#161b22" stroke="#3fb950" strokeWidth="1.5" />
          <text x="690" y="145" textAnchor="middle" fill="#3fb950" fontSize="11" fontWeight="700">NEXT FEATURE</text>
          <text x="690" y="162" textAnchor="middle" fill="#8b949e" fontSize="10">or complete</text>

          {/* Arrow: Evaluate → Fail (down) */}
          <line x1="460" y1="180" x2="460" y2="225" stroke="#f85149" strokeWidth="1.5" markerEnd="url(#arrow-red)" />
          <text x="475" y="210" fill="#f85149" fontSize="10" fontWeight="600">Fail</text>

          {/* Repair box */}
          <rect x="400" y="230" width="120" height="60" rx="10" fill="#161b22" stroke="#f85149" strokeWidth="1.5" />
          <text x="460" y="255" textAnchor="middle" fill="#f85149" fontSize="11" fontWeight="700">REPAIR</text>
          <text x="460" y="272" textAnchor="middle" fill="#8b949e" fontSize="10">Fix issues</text>

          {/* Arrow: Repair → Build (from REPAIR left, down, left, up into BUILD bottom) */}
          <path d="M400 260 L280 260 L280 185" stroke="#58a6ff" strokeWidth="1.5" strokeDasharray="6 3" markerEnd="url(#arrow-blue)" fill="none" />
          <text x="330" y="253" fill="#58a6ff" fontSize="10" fontWeight="600">Retry</text>

          {/* Arrow: Repair → Failed (straight right) */}
          <line x1="520" y1="260" x2="615" y2="260" stroke="#f85149" strokeWidth="1.5" strokeDasharray="4 3" markerEnd="url(#arrow-red)" />
          <text x="568" y="252" textAnchor="middle" fill="#484f58" fontSize="9">Max retries</text>

          {/* Failed box (same row as Repair) */}
          <rect x="620" y="230" width="140" height="60" rx="10" fill="#161b22" stroke="#484f58" strokeWidth="1.5" />
          <text x="690" y="255" textAnchor="middle" fill="#f85149" fontSize="11" fontWeight="700">FAILED</text>
          <text x="690" y="272" textAnchor="middle" fill="#8b949e" fontSize="10">or continue</text>

          {/* Legend */}
          <text x="40" y="312" fill="#484f58" fontSize="9">Each feature loops independently · Checkpointed after every stage · Resumable on crash</text>
        </svg>
      </div>
    </section>
  );
}

function PromptEditor({
  testid,
  title,
  value,
  onChange,
}: {
  testid: string;
  title: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <section className="prompt-editor" data-testid={testid}>
      <div className="prompt-editor-header">
        <h3>{title}</h3>
      </div>
      <textarea data-testid={`${testid}-textarea`} value={value} onChange={(event) => onChange(event.target.value)} rows={9} />
    </section>
  );
}

function StatCard({
  testid,
  label,
  value,
  detail,
}: {
  testid: string;
  label: string;
  value: string;
  detail: string;
}) {
  return (
    <article className="stat-card" data-testid={testid}>
      <span>{label}</span>
      <strong>{value}</strong>
      <p>{detail}</p>
    </article>
  );
}

function StageRow({ testid, heading, text }: { testid?: string; heading: string; text: string }) {
  return (
    <div className="stage-row" data-testid={testid}>
      <strong>{heading}</strong>
      <span>{text}</span>
    </div>
  );
}

function buildUpdatedProfile(
  profile: WorkspaceProfile,
  editors: EditorState,
  defaults: PromptSnapshot | null,
): WorkspaceProfile {
  return {
    ...profile,
    preferred_config_path: editors.configPath.trim() || null,
    request_draft: editors.requestDraft,
    prompt_overrides: currentOverrides(editors, defaults),
  };
}

function currentOverrides(
  editors: EditorState | null,
  defaults: PromptSnapshot | null,
): PromptOverrides {
  if (!editors) {
    return emptyOverrides;
  }

  if (!defaults) {
    return {
      planner: editors.plannerPrompt.trim() ? editors.plannerPrompt : null,
      builder: editors.builderPrompt.trim() ? editors.builderPrompt : null,
      evaluator: editors.evaluatorPrompt.trim() ? editors.evaluatorPrompt : null,
    };
  }

  return {
    planner:
      editors.plannerPrompt !== defaults.planner ? editors.plannerPrompt : null,
    builder:
      editors.builderPrompt !== defaults.builder ? editors.builderPrompt : null,
    evaluator:
      editors.evaluatorPrompt !== defaults.evaluator
        ? editors.evaluatorPrompt
        : null,
  };
}

function upsertProfile(
  profiles: WorkspaceProfile[],
  nextProfile: WorkspaceProfile,
): WorkspaceProfile[] {
  const existing = profiles.filter(
    (profile) => profile.workspace_path !== nextProfile.workspace_path,
  );
  return [nextProfile, ...existing].sort(
    (left, right) =>
      new Date(right.last_opened_at).getTime() -
      new Date(left.last_opened_at).getTime(),
  );
}

function basename(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : path;
}

function formatDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value));
}

function readError(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Unexpected error";
}
