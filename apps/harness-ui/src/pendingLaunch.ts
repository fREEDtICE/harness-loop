import type { WorkspacePayload, WorkspaceRunSummary } from "./types";

export const PENDING_RUN_PREFIX = "pending:";

export type PendingLaunch = {
  workspacePath: string;
  startedAt: string;
  run: WorkspaceRunSummary;
};

export function createPendingLaunch(
  workspacePath: string,
  requestDraft: string,
): PendingLaunch {
  const startedAt = new Date().toISOString();
  return {
    workspacePath,
    startedAt,
    run: {
      run_root: `${PENDING_RUN_PREFIX}${workspacePath}:${startedAt}`,
      run_title: truncateTitle(requestDraft, 50),
      created_at: startedAt,
      updated_at: startedAt,
      lifecycle: "running",
      final_status: null,
      current_feature_index: 0,
      total_features: 0,
      awaiting_feature_confirmation: false,
      active_stage: null,
    },
  };
}

export function mergePendingLaunch(
  workspace: WorkspacePayload | null,
  pendingLaunch: PendingLaunch | null,
): WorkspacePayload | null {
  if (
    !workspace
    || !pendingLaunch
    || workspace.record.workspace_path !== pendingLaunch.workspacePath
  ) {
    return workspace;
  }
  if (hasMaterializedRun(workspace, pendingLaunch)) {
    return workspace;
  }

  return {
    ...workspace,
    runs: [
      pendingLaunch.run,
      ...workspace.runs.filter((run) => run.run_root !== pendingLaunch.run.run_root),
    ],
  };
}

export function hasMaterializedRun(
  workspace: WorkspacePayload,
  pendingLaunch: PendingLaunch,
): boolean {
  const pendingStart = Date.parse(pendingLaunch.startedAt);
  const threshold = Number.isFinite(pendingStart)
    ? pendingStart - 1000
    : Number.NEGATIVE_INFINITY;
  return workspace.runs.some((run) => Date.parse(run.created_at) >= threshold)
    || (
      workspace.current_run !== null
      && Date.parse(workspace.current_run.created_at) >= threshold
    );
}

export function isPendingLaunchRun(run: WorkspaceRunSummary): boolean {
  return run.run_root.startsWith(PENDING_RUN_PREFIX);
}

function truncateTitle(text: string, maxChars: number): string {
  const firstLine = text.trim().split("\n")[0] ?? "";
  if (firstLine.length <= maxChars) {
    return firstLine;
  }
  return `${firstLine.slice(0, maxChars)}…`;
}
