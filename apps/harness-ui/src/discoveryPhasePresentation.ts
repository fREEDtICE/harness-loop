import type { TFunction } from "i18next";
import type {
  WorkspaceDiscoveryPayload,
  WorkspaceDiscoveryPhase,
} from "./types";

type DiscoveryPhasePresentationKeys = {
  phaseLabel: string;
  timelineLabel: string;
};

const DISCOVERY_PHASE_PRESENTATION_KEYS: Record<
  WorkspaceDiscoveryPhase,
  DiscoveryPhasePresentationKeys
> = {
  idle: {
    phaseLabel: "discovery.idle",
    timelineLabel: "discovery.timelineIdle",
  },
  scanning: {
    phaseLabel: "discovery.scanning",
    timelineLabel: "discovery.timelineScanning",
  },
  reusing_cached_profile: {
    phaseLabel: "discovery.reusingCachedProfile",
    timelineLabel: "discovery.timelineReusingCachedProfile",
  },
  polishing: {
    phaseLabel: "discovery.polishing",
    timelineLabel: "discovery.timelinePolishing",
  },
  using_fallback_profile: {
    phaseLabel: "discovery.usingFallbackProfile",
    timelineLabel: "discovery.timelineUsingFallbackProfile",
  },
  ready: {
    phaseLabel: "discovery.ready",
    timelineLabel: "discovery.timelineReady",
  },
  failed: {
    phaseLabel: "discovery.failed",
    timelineLabel: "discovery.timelineFailed",
  },
};

export function getDiscoveryPhasePresentation(
  phase: WorkspaceDiscoveryPhase,
  t: TFunction,
) {
  const keys = DISCOVERY_PHASE_PRESENTATION_KEYS[phase];
  return {
    phaseLabel: t(keys.phaseLabel),
    timelineLabel: t(keys.timelineLabel),
  };
}

export function getDiscoveryTimelineProgress(
  discovery: WorkspaceDiscoveryPayload | null | undefined,
  t: TFunction,
): string {
  if (!discovery) {
    return t("app.startingRun");
  }

  return getDiscoveryPhasePresentation(discovery.status.current_phase, t)
    .timelineLabel;
}
