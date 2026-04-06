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
const compiledDir = path.join(scriptsDir, ".compiled", "pending-launch");

let loadedModules = null;

function ensureCompiledModules() {
  if (loadedModules) {
    return loadedModules;
  }

  fs.rmSync(compiledDir, { recursive: true, force: true });
  execFileSync(
    "pnpm",
    ["exec", "tsc", "-p", "scripts/tsconfig.pending-launch-contract.json"],
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

  require(path.join(compiledDir, "i18n", "index.js"));

  loadedModules = {
    React: require("react"),
    renderToStaticMarkup: require("react-dom/server").renderToStaticMarkup,
    WorkspaceTimeline: require(path.join(compiledDir, "WorkspaceTimeline.js")).default,
    pendingLaunch: require(path.join(compiledDir, "pendingLaunch.js")),
  };
  return loadedModules;
}

function baseWorkspacePayload() {
  return {
    record: {
      workspace_path: "/tmp/workspace",
      display_name: "workspace",
      last_opened_at: "2026-04-04T00:00:00.000Z",
      pinned: false,
    },
    config_path: "/tmp/workspace/.loopsmith/config.toml",
    prompts: null,
    discovery: null,
    runs: [],
    current_run: null,
    config_error: null,
  };
}

test("pending launch is merged into the workspace timeline immediately", () => {
  const { React, renderToStaticMarkup, WorkspaceTimeline, pendingLaunch } =
    ensureCompiledModules();
  const workspace = baseWorkspacePayload();
  const pending = pendingLaunch.createPendingLaunch(
    workspace.record.workspace_path,
    "Create the billing dashboard and ship it.",
  );

  const merged = pendingLaunch.mergePendingLaunch(workspace, pending);

  assert.equal(merged.runs.length, 1);
  assert.equal(pendingLaunch.isPendingLaunchRun(merged.runs[0]), true);

  const markup = renderToStaticMarkup(
    React.createElement(WorkspaceTimeline, {
      workspace: merged,
      isRunning: true,
      onSelectRun() {},
      onNewRun() {},
      onProjectSettings() {},
      onResumeRun() {},
    }),
  );

  assert.match(markup, /Starting harness run\./);
  assert.doesNotMatch(markup, /No runs yet/);
  assert.doesNotMatch(markup, /Resume/);
});

test("pending launch renders live discovery progress when status is available", () => {
  const { React, renderToStaticMarkup, WorkspaceTimeline, pendingLaunch } =
    ensureCompiledModules();
  const workspace = {
    ...baseWorkspacePayload(),
    discovery: {
      status: {
        workspace_path: "/tmp/workspace",
        scan_path: "/tmp/workspace/.loopsmith/discovery/scan.json",
        profile_path: "/tmp/workspace/.loopsmith/discovery/profile.json",
        workspace_fingerprint: "scan-fingerprint",
        profile_fingerprint: null,
        last_scanned_at: "2026-04-04T00:00:00.000Z",
        last_refreshed_at: null,
        last_refresh_error: null,
        used_fallback_profile: false,
        current_phase: "polishing",
      },
      profile_summary: null,
    },
  };
  const pending = pendingLaunch.createPendingLaunch(
    workspace.record.workspace_path,
    "Create the billing dashboard and ship it.",
  );
  const merged = pendingLaunch.mergePendingLaunch(workspace, pending);

  const markup = renderToStaticMarkup(
    React.createElement(WorkspaceTimeline, {
      workspace: merged,
      isRunning: true,
      onSelectRun() {},
      onNewRun() {},
      onProjectSettings() {},
      onResumeRun() {},
    }),
  );

  assert.match(markup, /Discovery: refreshing workspace profile/);
  assert.doesNotMatch(markup, /Starting harness run\./);
});

test("materialized runs replace the pending launch placeholder", () => {
  const { pendingLaunch } = ensureCompiledModules();
  const workspace = baseWorkspacePayload();
  const pending = pendingLaunch.createPendingLaunch(
    workspace.record.workspace_path,
    "Create the billing dashboard and ship it.",
  );
  const createdAt = new Date(Date.parse(pending.startedAt) + 2_000).toISOString();
  const materializedWorkspace = {
    ...workspace,
    runs: [
      {
        run_root: "/tmp/workspace/.loopsmith-runs/run-001",
        run_title: "Create the billing dashboard and ship it.",
        created_at: createdAt,
        updated_at: createdAt,
        lifecycle: "running",
        final_status: null,
        current_feature_index: 0,
        total_features: 1,
        active_stage: null,
      },
    ],
  };

  assert.equal(
    pendingLaunch.hasMaterializedRun(materializedWorkspace, pending),
    true,
  );

  const merged = pendingLaunch.mergePendingLaunch(materializedWorkspace, pending);

  assert.equal(merged.runs.length, 1);
  assert.equal(merged.runs[0].run_root, "/tmp/workspace/.loopsmith-runs/run-001");
  assert.equal(pendingLaunch.isPendingLaunchRun(merged.runs[0]), false);
});

test("pending launch is not injected into a different workspace payload", () => {
  const { pendingLaunch } = ensureCompiledModules();
  const pending = pendingLaunch.createPendingLaunch(
    "/tmp/workspace-a",
    "Create the billing dashboard and ship it.",
  );
  const otherWorkspace = {
    ...baseWorkspacePayload(),
    record: {
      ...baseWorkspacePayload().record,
      workspace_path: "/tmp/workspace-b",
    },
  };

  const merged = pendingLaunch.mergePendingLaunch(otherWorkspace, pending);

  assert.equal(merged.runs.length, 0);
});
