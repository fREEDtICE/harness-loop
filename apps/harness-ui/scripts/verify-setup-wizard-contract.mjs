import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const uiRoot = path.resolve(__dirname, "..");
const srcRoot = path.join(uiRoot, "src");

function read(relativePath) {
  return fs.readFileSync(path.join(uiRoot, relativePath), "utf8");
}

function translationAtKey(obj, dottedKey) {
  return dottedKey.split(".").reduce((current, key) => {
    if (current && typeof current === "object" && key in current) {
      return current[key];
    }
    return undefined;
  }, obj);
}

function assert(condition, message, failures) {
  if (!condition) {
    failures.push(message);
  }
}

const setupWizard = read(path.join("src", "SetupWizard.tsx"));
const settingsPanel = read(path.join("src", "SettingsPanel.tsx"));
const en = JSON.parse(read(path.join("src", "i18n", "en.json")));
const zh = JSON.parse(read(path.join("src", "i18n", "zh.json")));

const scanStepKeys = [
  "setup.scanSteps.detectShellEnvironment",
  "setup.scanSteps.detectNode",
  "setup.scanSteps.detectCodex",
  "setup.scanSteps.detectClaude",
  "setup.scanSteps.detectGemini",
  "setup.scanSteps.resolveVersions",
];

const removedHardCodedSteps = [
  "Detecting shell environment…",
  "Looking for Node.js…",
  "Checking Codex CLI…",
  "Checking Claude Code…",
  "Checking Gemini CLI…",
  "Resolving versions…",
];

const failures = [];

assert(
  setupWizard.includes('className="setup-overlay"') &&
    setupWizard.includes("event.stopPropagation();") &&
    setupWizard.includes("if (event.target === event.currentTarget)") &&
    setupWizard.includes("onCancel?.();"),
  "SetupWizard overlay does not keep clicks from bubbling or dismiss itself only on backdrop clicks.",
  failures,
);

assert(
  setupWizard.includes('className="setup-panel" onClick={(event) => event.stopPropagation()}'),
  "SetupWizard panel no longer contains the explicit stopPropagation guard.",
  failures,
);

assert(
  settingsPanel.includes('className="settings-overlay" data-testid="settings-overlay" onClick={onClose}') &&
    settingsPanel.includes("<SetupWizard") &&
    settingsPanel.includes("onCancel={() => setShowWizard(false)}"),
  "SettingsPanel no longer hosts SetupWizard through the expected modal path.",
  failures,
);

for (const hardCodedStep of removedHardCodedSteps) {
  assert(
    !setupWizard.includes(hardCodedStep),
    `SetupWizard still contains hard-coded scan text: ${hardCodedStep}`,
    failures,
  );
}

for (const key of scanStepKeys) {
  assert(
    typeof translationAtKey(en, key) === "string",
    `English translations are missing ${key}.`,
    failures,
  );
  assert(
    typeof translationAtKey(zh, key) === "string",
    `Chinese translations are missing ${key}.`,
    failures,
  );
  assert(
    setupWizard.includes(`"${key}"`) || setupWizard.includes(`'${key}'`),
    `SetupWizard does not include ${key} in the scan-step key list.`,
    failures,
  );
}

assert(
  setupWizard.includes("{t(stepKey)}"),
  "SetupWizard no longer renders scan steps through the step-key map.",
  failures,
);

const result = {
  script: path.relative(process.cwd(), __filename),
  src_root: path.relative(process.cwd(), srcRoot),
  checks: [
    "Wizard overlay stops propagation and only backdrop clicks call onCancel.",
    "Wizard panel stops propagation within the settings overlay.",
    "SettingsPanel hosts Quick Setup through SetupWizard with a local onCancel handler.",
    "SetupWizard no longer contains the legacy hard-coded scan strings.",
    "All setup.scanSteps keys exist in both English and Chinese locales and are referenced from SetupWizard.",
  ],
  status: failures.length === 0 ? "passed" : "failed",
  failures,
};

console.log(JSON.stringify(result, null, 2));

if (failures.length > 0) {
  process.exitCode = 1;
}
