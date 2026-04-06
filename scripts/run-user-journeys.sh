#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

cd "$REPO_ROOT"

resolve_live_lane() {
  if [ "${CODEX_LIVE_E2E+x}" = "x" ]; then
    case "${CODEX_LIVE_E2E}" in
      1)
        echo "CODEX_LIVE_E2E=1; forcing live journeys"
        return 0
        ;;
      0)
        echo "CODEX_LIVE_E2E=0; forcing live journey skip mode"
        return 1
        ;;
      *)
        echo "Unsupported CODEX_LIVE_E2E=${CODEX_LIVE_E2E}; expected 0 or 1" >&2
        exit 2
        ;;
    esac
  fi

  if ! command -v codex >/dev/null 2>&1; then
    echo "Codex CLI not found; live journeys will execute in skip mode"
    return 1
  fi

  if codex login status >/dev/null 2>&1; then
    echo "Detected Codex CLI login; enabling live journeys automatically"
    return 0
  fi

  echo "Codex CLI is installed but not logged in; live journeys will execute in skip mode"
  return 1
}

echo "==> Running deterministic user journeys"
cargo test --test run_smoke -- --nocapture
cargo test --test discovery_smoke -- --nocapture

if command -v pnpm >/dev/null 2>&1 && [ -d "$REPO_ROOT/apps/harness-ui/node_modules" ]; then
  echo "==> Running frontend pending-launch contracts"
  pnpm --dir "$REPO_ROOT/apps/harness-ui" test:pending-launch-contract
  echo "==> Running frontend run-detail streaming contracts"
  pnpm --dir "$REPO_ROOT/apps/harness-ui" test:run-detail-streaming
  echo "==> Running frontend run-selection freshness contracts"
  pnpm --dir "$REPO_ROOT/apps/harness-ui" test:run-selection-freshness
  echo "==> Verifying frontend setup wizard contracts"
  pnpm --dir "$REPO_ROOT/apps/harness-ui" verify:setup-wizard-contract
else
  echo "Skipping frontend UI contracts; pnpm or frontend dependencies are unavailable"
fi

echo "==> Running live user journeys"
if resolve_live_lane; then
  CODEX_LIVE_E2E=1 cargo test --test live_codex -- --nocapture
else
  CODEX_LIVE_E2E=0 cargo test --test live_codex -- --nocapture
fi
