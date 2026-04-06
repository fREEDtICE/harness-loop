import test from "node:test";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import path from "node:path";
import fs from "node:fs";

const require = createRequire(import.meta.url);
const scriptsDir = path.dirname(fileURLToPath(import.meta.url));
const appDir = path.resolve(scriptsDir, "..");
const compiledDir = path.join(scriptsDir, ".compiled", "discovery-ui");

let loadedModules = null;

function ensureCompiledModules() {
  if (loadedModules) {
    return loadedModules;
  }

  fs.rmSync(compiledDir, { recursive: true, force: true });
  execFileSync(
    "pnpm",
    ["exec", "tsc", "-p", "scripts/tsconfig.discovery-ui-contract.json"],
    { cwd: appDir, stdio: "inherit" },
  );
  fs.writeFileSync(
    path.join(compiledDir, "package.json"),
    JSON.stringify({ type: "commonjs" }),
  );

  const localStorageState = new Map();
  globalThis.localStorage = {
    getItem(key) {
      return localStorageState.get(key) ?? null;
    },
    setItem(key, value) {
      localStorageState.set(key, String(value));
    },
    removeItem(key) {
      localStorageState.delete(key);
    },
    clear() {
      localStorageState.clear();
    },
  };
  Object.defineProperty(globalThis, "navigator", {
    value: { language: "en-US" },
    configurable: true,
  });

  const i18n = require(path.join(compiledDir, "i18n", "index.js")).default;

  loadedModules = {
    React: require("react"),
    i18n,
    renderToStaticMarkup: require("react-dom/server").renderToStaticMarkup,
    DiscoveryStatusPanel: require(path.join(compiledDir, "DiscoveryStatusPanel.js"))
      .default,
    WorkspaceTimeline: require(path.join(compiledDir, "WorkspaceTimeline.js")).default,
    pendingLaunch: require(path.join(compiledDir, "pendingLaunch.js")),
    discoveryPhasePresentation: require(
      path.join(compiledDir, "discoveryPhasePresentation.js"),
    ),
    types: require(path.join(compiledDir, "types.js")),
  };
  return loadedModules;
}

function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function baseWorkspacePayload(discovery) {
  return {
    record: {
      workspace_path: "/tmp/workspace",
      display_name: "workspace",
      last_opened_at: "2026-04-04T00:00:00.000Z",
      pinned: false,
    },
    config_path: "/tmp/workspace/.loopsmith/config.toml",
    prompts: null,
    discovery,
    runs: [],
    current_run: null,
    config_error: null,
  };
}

function discoveryFixture(phase) {
  const base = {
    workspace_path: "/tmp/workspace",
    scan_path: "/tmp/workspace/.loopsmith/discovery/scan.json",
    profile_path: "/tmp/workspace/.loopsmith/discovery/profile.json",
    workspace_fingerprint: `workspace-${phase}`,
    profile_fingerprint: `profile-${phase}`,
    last_scanned_at: "2026-04-04T00:00:00.000Z",
    last_refreshed_at: "2026-04-04T00:00:05.000Z",
    last_refresh_error: null,
    used_fallback_profile: false,
    current_phase: phase,
  };

  if (phase === "idle" || phase === "scanning") {
    base.profile_fingerprint = null;
    base.last_refreshed_at = null;
  }
  if (phase === "using_fallback_profile") {
    base.used_fallback_profile = true;
    base.last_refresh_error = "worker failed; reused cached profile";
  }
  if (phase === "failed") {
    base.profile_fingerprint = null;
    base.last_refresh_error = "worker failed";
  }

  return {
    status: base,
    profile_summary:
      phase === "failed" ? null : `Discovery summary for ${phase}`,
  };
}

test("shared discovery contract covers every current phase", () => {
  const {
    React,
    i18n,
    renderToStaticMarkup,
    DiscoveryStatusPanel,
    WorkspaceTimeline,
    pendingLaunch,
    discoveryPhasePresentation,
    types,
  } = ensureCompiledModules();

  assert.deepEqual(types.WORKSPACE_DISCOVERY_PHASES, [
    "idle",
    "scanning",
    "reusing_cached_profile",
    "polishing",
    "using_fallback_profile",
    "ready",
    "failed",
  ]);

  for (const phase of types.WORKSPACE_DISCOVERY_PHASES) {
    const discovery = discoveryFixture(phase);
    const workspace = baseWorkspacePayload(discovery);
    const pending = pendingLaunch.createPendingLaunch(
      workspace.record.workspace_path,
      "Refresh the discovery UI contract.",
    );
    const merged = pendingLaunch.mergePendingLaunch(workspace, pending);
    const presentation = discoveryPhasePresentation.getDiscoveryPhasePresentation(
      phase,
      i18n.t.bind(i18n),
    );

    const timelineMarkup = renderToStaticMarkup(
      React.createElement(WorkspaceTimeline, {
        workspace: merged,
        isRunning: true,
        onSelectRun() {},
        onNewRun() {},
        onProjectSettings() {},
        onResumeRun() {},
      }),
    );
    const settingsMarkup = renderToStaticMarkup(
      React.createElement(DiscoveryStatusPanel, { discovery }),
    );

    assert.match(
      timelineMarkup,
      new RegExp(escapeRegExp(presentation.timelineLabel)),
    );
    assert.match(
      settingsMarkup,
      new RegExp(escapeRegExp(presentation.phaseLabel)),
    );
    assert.match(settingsMarkup, /\/tmp\/workspace\/\.loopsmith\/discovery\/scan\.json/);
    assert.match(settingsMarkup, /\/tmp\/workspace\/\.loopsmith\/discovery\/profile\.json/);
  }
});
