export interface SelectedRunInspectTarget {
  workspacePath: string | null;
  runRoot: string | null;
  generation: number;
}

export interface InspectRunRequest {
  workspacePath: string;
  runRoot: string;
  generation: number;
}

export function createSelectedRunInspectTarget(): SelectedRunInspectTarget {
  return { workspacePath: null, runRoot: null, generation: 0 };
}

export function updateSelectedRunInspectTarget(
  current: SelectedRunInspectTarget,
  workspacePath: string | null,
  runRoot: string | null,
): SelectedRunInspectTarget {
  if (current.workspacePath === workspacePath && current.runRoot === runRoot) {
    return current;
  }
  return { workspacePath, runRoot, generation: current.generation + 1 };
}

export function createInspectRunRequest(
  target: SelectedRunInspectTarget,
  runRoot: string,
): InspectRunRequest | null {
  if (!target.workspacePath || target.runRoot !== runRoot) {
    return null;
  }
  return {
    workspacePath: target.workspacePath,
    runRoot: target.runRoot,
    generation: target.generation,
  };
}

export function canCommitInspectRunResponse(
  target: SelectedRunInspectTarget,
  request: InspectRunRequest,
  responseRunRoot: string,
): boolean {
  return (
    target.generation === request.generation &&
    target.runRoot === responseRunRoot
  );
}

export function invalidateInspectRunRequestIfCurrent(
  target: SelectedRunInspectTarget,
  workspacePath: string,
  runRoot: string,
): SelectedRunInspectTarget {
  if (target.workspacePath === workspacePath && target.runRoot === runRoot) {
    return { ...target, generation: target.generation + 1 };
  }
  return target;
}
