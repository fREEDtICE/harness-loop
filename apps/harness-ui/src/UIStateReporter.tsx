import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef } from "react";

export type UIStateSnapshot = {
  timestamp: string;
  panels: {
    sidebar: {
      visible: true;
      workspaceCount: number;
      workspaces: {
        testid: string;
        displayName: string;
        workspacePath: string;
        selected: boolean;
        pinned: boolean;
        lastRunRoot: string | null;
      }[];
    };
    launch: {
      visible: boolean;
      title: string | null;
      configPath: string;
      requestDraft: string;
      hasConfigError: boolean;
      configError: string | null;
      startButtonDisabled: boolean;
      isRunning: boolean;
    };
    promptStudio: {
      visible: boolean;
      editors: {
        planner: { hasContent: boolean; charCount: number };
        builder: { hasContent: boolean; charCount: number };
        evaluator: { hasContent: boolean; charCount: number };
      } | null;
    };
    history: {
      visible: boolean;
      runCount: number;
      runs: {
        testid: string;
        name: string;
        lifecycle: string;
        finalStatus: string | null;
        currentFeatureIndex: number;
        totalFeatures: number;
        activeStage: string | null;
        updatedAt: string;
      }[];
    };
    monitor: {
      visible: boolean;
      runRoot: string | null;
      runId: string | null;
      lifecycle: string | null;
      finalStatus: string | null;
      progress: string | null;
      activeStage: string | null;
      featureCount: number;
      features: {
        testid: string;
        featureId: string;
        title: string;
        status: string;
        phase: string;
        repairAttemptsUsed: number;
        stageCount: number;
        stages: {
          stage: string;
          attempt: number;
          status: string;
        }[];
      }[];
      planStage: {
        attempt: number;
        status: string;
      } | null;
    };
  };
  globalState: {
    selectedWorkspacePath: string | null;
    statusMessage: string;
    errorMessage: string | null;
    isRunning: boolean;
    isPending: boolean;
  };
};

export type UIStateReporterProps = {
  workspaces: {
    workspace_path: string;
    display_name: string;
    pinned: boolean;
  }[];
  selectedWorkspacePath: string | null;
  workspace: {
    record: {
      workspace_path: string;
      display_name: string;
    };
    config_path: string;
    prompts: {
      defaults: { planner: string; builder: string; evaluator: string };
      effective: { planner: string; builder: string; evaluator: string };
    } | null;
    runs: {
      run_root: string;
      created_at: string;
      updated_at: string;
      lifecycle: string;
      final_status: string | null;
      current_feature_index: number;
      total_features: number;
      active_stage: { stage: string; attempt: number } | null;
    }[];
    current_run: {
      run_id: string;
      run_root: string;
      lifecycle: string;
      final_status: string | null;
      current_feature_index: number;
      active_stage: {
        stage: string;
        attempt: number;
        feature_id: string | null;
      } | null;
      plan_stage: { attempt: number; status: string } | null;
      features: {
        feature_id: string;
        title: string;
        status: string;
        phase: string;
        repair_attempts_used: number;
        stages: { stage: string; attempt: number; status: string }[];
      }[];
    } | null;
    config_error: string | null;
  } | null;
  editors: {
    requestDraft: string;
    plannerPrompt: string;
    builderPrompt: string;
    evaluatorPrompt: string;
  } | null;
  statusMessage: string;
  errorMessage: string | null;
  isRunning: boolean;
  isPending: boolean;
};

function basename(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : path;
}

function buildSnapshot(props: UIStateReporterProps): UIStateSnapshot {
  const {
    workspaces,
    selectedWorkspacePath,
    workspace,
    editors,
    statusMessage,
    errorMessage,
    isRunning,
    isPending,
  } = props;

  const activeRun = workspace?.current_run ?? null;
  const hasWorkspace = workspace !== null && editors !== null;

  return {
    timestamp: new Date().toISOString(),
    panels: {
      sidebar: {
        visible: true,
        workspaceCount: workspaces.length,
        workspaces: workspaces.map((p) => ({
          testid: `sidebar-workspace-${basename(p.workspace_path)}`,
          displayName: p.display_name,
          workspacePath: p.workspace_path,
          selected: selectedWorkspacePath === p.workspace_path,
          pinned: p.pinned,
          lastRunRoot: null,
        })),
      },
      launch: {
        visible: hasWorkspace,
        title: workspace?.record.display_name ?? null,
        configPath: workspace?.config_path ?? "",
        requestDraft: editors?.requestDraft ?? "",
        hasConfigError: workspace?.config_error !== null && workspace?.config_error !== undefined,
        configError: workspace?.config_error ?? null,
        startButtonDisabled: isRunning,
        isRunning,
      },
      promptStudio: {
        visible: hasWorkspace,
        editors: editors
          ? {
              planner: {
                hasContent: editors.plannerPrompt.trim().length > 0,
                charCount: editors.plannerPrompt.length,
              },
              builder: {
                hasContent: editors.builderPrompt.trim().length > 0,
                charCount: editors.builderPrompt.length,
              },
              evaluator: {
                hasContent: editors.evaluatorPrompt.trim().length > 0,
                charCount: editors.evaluatorPrompt.length,
              },
            }
          : null,
      },
      history: {
        visible: hasWorkspace,
        runCount: workspace?.runs.length ?? 0,
        runs: (workspace?.runs ?? []).map((run) => ({
          testid: `history-run-${basename(run.run_root)}`,
          name: basename(run.run_root),
          lifecycle: run.lifecycle,
          finalStatus: run.final_status,
          currentFeatureIndex: run.current_feature_index,
          totalFeatures: run.total_features,
          activeStage: run.active_stage
            ? `${run.active_stage.stage} attempt ${run.active_stage.attempt}`
            : null,
          updatedAt: run.updated_at,
        })),
      },
      monitor: {
        visible: activeRun !== null,
        runRoot: activeRun ? basename(activeRun.run_root) : null,
        runId: activeRun?.run_id ?? null,
        lifecycle: activeRun?.lifecycle ?? null,
        finalStatus: activeRun?.final_status ?? null,
        progress: activeRun
          ? `${activeRun.current_feature_index}/${activeRun.features.length}`
          : null,
        activeStage: activeRun?.active_stage
          ? `${activeRun.active_stage.stage} attempt ${activeRun.active_stage.attempt}`
          : null,
        featureCount: activeRun?.features.length ?? 0,
        features: (activeRun?.features ?? []).map((f) => {
          const liveStage = activeRun?.active_stage &&
            activeRun.active_stage.feature_id === f.feature_id &&
            !f.stages.some(
              (s) =>
                s.stage === activeRun.active_stage!.stage &&
                s.attempt === activeRun.active_stage!.attempt,
            )
            ? {
                stage: activeRun.active_stage.stage,
                attempt: activeRun.active_stage.attempt,
                status: "running" as const,
              }
            : null;
          const stages = liveStage ? [...f.stages, liveStage] : f.stages;
          return {
            testid: `monitor-feature-${f.feature_id}`,
            featureId: f.feature_id,
            title: f.title,
            status: f.status,
            phase: f.phase,
            repairAttemptsUsed: f.repair_attempts_used,
            stageCount: stages.length,
            stages: stages.map((s) => ({
              stage: s.stage,
              attempt: s.attempt,
              status: s.status,
            })),
          };
        }),
        planStage: activeRun?.plan_stage
          ? {
              attempt: activeRun.plan_stage.attempt,
              status: activeRun.plan_stage.status,
            }
          : activeRun?.active_stage?.stage === "plan"
            ? {
                attempt: activeRun.active_stage.attempt,
                status: "running",
              }
            : null,
      },
    },
    globalState: {
      selectedWorkspacePath,
      statusMessage,
      errorMessage,
      isRunning,
      isPending,
    },
  };
}

export default function UIStateReporter(props: UIStateReporterProps) {
  const timerRef = useRef<number | null>(null);
  const lastJsonRef = useRef<string>("");

  useEffect(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
    }

    timerRef.current = window.setTimeout(() => {
      const snapshot = buildSnapshot(props);
      const json = JSON.stringify(snapshot, null, 2);

      if (json === lastJsonRef.current) {
        return;
      }
      lastJsonRef.current = json;

      invoke("write_ui_state", { json }).catch((err) => {
        console.warn("[UIStateReporter] failed to write state:", err);
      });
    }, 300);

    return () => {
      if (timerRef.current !== null) {
        window.clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  });

  return null;
}
