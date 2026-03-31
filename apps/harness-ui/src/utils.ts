import type { EditorState, PromptOverrides, PromptSnapshot } from "./types";

export const emptyOverrides: PromptOverrides = {
  planner: null,
  builder: null,
  evaluator: null,
};

export function currentOverrides(
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

export function basename(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments.length > 0 ? segments[segments.length - 1] : path;
}

export function formatDate(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(value));
}

export function readError(error: unknown): string {
  if (typeof error === "string") {
    return error;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "Unexpected error";
}
