# codex-harness-rs

Rust scaffold for a long-running application development harness built around a disposable coding worker and durable on-disk control artifacts.

## What is here

- `src/main.rs`: CLI entrypoint with `run`, `resume`, and `inspect`.
- `crates/harness-core`: controller, config loading, backlog state, artifact store, workspace isolation, runtime planning, service supervision, and verification execution.
- `crates/harness-worker-codex`: production Codex CLI adapter.
- `crates/harness-worker-simulated`: deterministic offline worker for smoke tests and local harness development.
- `config/example.toml`: safe offline config using the simulated worker.
- `config/codex-cli.toml`: default real Codex CLI config with git worktree isolation enabled.

## Design stance

The outer loop stays in Rust:

- The controller owns run state, retries, stop conditions, and checkpointing.
- Workers are disposable and resumable.
- Request, plan, contracts, QA, verification evidence, and runtime artifacts live on disk.
- Runtime services can be started, readiness-checked, and torn down outside the worker.
- Verification commands run outside the worker and can deterministically fail a feature even if the evaluator would otherwise pass it.
- Repair can resume from the previous Codex session when a thread id was captured.

## Current run model

Each run creates `.runs/<uuid>/` with:

- `request.md`
- `plan.json`
- `runtime-plan.json`
- `run-state.json`
- `manifest.json`
- `runtime/services/<service>/` for supervised service logs and lifecycle records when runtime supervision is enabled
- `runtime/stacks/<stack>/` for command-driven stack orchestration logs and lifecycle records when runtime supervision is enabled
- `worker/` for plan-stage prompts, outputs, logs, and result metadata
- `features/<NN>-<feature-id>/` for per-feature contracts, builder handoffs, QA reports, runtime verification evidence, and worker artifacts

The controller executes:

1. `plan`
2. `build`
3. `evaluate`
4. `repair -> evaluate` until pass or `runtime.max_repair_attempts` is exhausted

Feature progress is persisted in `run-state.json`, so `resume` can continue from the last incomplete phase.

## Configs

`config/example.toml` is intentionally safe for offline harness development:

- `worker.kind = "simulated"`
- `runtime.supervision.enabled = false`
- `evaluator.require_screenshots = false`
- verification commands default to `["/usr/bin/env", "true"]`
- optional `[worker.planner]` lets you route only the plan stage through a different worker

Use that when developing the harness itself without the real Codex worker.

`config/codex-cli.toml` is the default production-oriented template:

- `worker.kind = "codex_cli"`
- `workspace.isolation = "git_worktree"`
- `runtime.supervision.enabled = true`
- supports both per-service processes and command-driven runtime stacks
- optional `[worker.planner]` can run live Codex planning while build and evaluation stay on another worker
- `evaluator.require_screenshots = false` until repo-specific screenshot commands are configured
- evaluator commands are real repo checks such as `cargo test` and `pnpm test:e2e`

If `--config` is omitted, the CLI now defaults to this file. Use it when you want the harness to drive a real target repository.

## Quick start

Run the full outer loop with the default real Codex config:

```bash
cargo run -- run \
  --config config/codex-cli.toml \
  --workspace /absolute/path/to/target-app \
  --request-file /absolute/path/to/request.md
```

Inspect a completed or partial run:

```bash
cargo run -- inspect \
  --config config/codex-cli.toml \
  --run-root .runs/<run-id>
```

Resume an interrupted run:

```bash
cargo run -- resume \
  --config config/codex-cli.toml \
  --run-root .runs/<run-id>
```

## CLI parameters

All path arguments may be absolute or relative. Relative paths are resolved from the current working directory before the run starts.

`run`

- `--config`: optional path to the TOML config file. Defaults to `config/codex-cli.toml`.
- `--workspace`: required path to the source workspace the harness should copy or isolate for execution.
- `--request-file`: required path to the text file containing the user request.
- `--feature-limit`: optional per-run override for `runtime.feature_limit`. Values below `1` are clamped to `1`.

`resume`

- `--config`: optional path to the TOML config file. Defaults to `config/codex-cli.toml`.
- `--run-root`: required path to an existing run directory under `.runs/` or another configured storage location.

`inspect`

- `--config`: optional path to the TOML config file. Defaults to `config/codex-cli.toml`.
- `--run-root`: required path to an existing run directory to inspect without resuming execution.

## Verification evidence

Before each evaluate turn, the harness executes `evaluator.commands` in the execution workspace and stores evidence under the feature root:

- `features/<NN>-<feature-id>/runtime/verification/evaluate-01/report.json`
- `features/<NN>-<feature-id>/runtime/verification/evaluate-01/check-01.stdout.log`
- `features/<NN>-<feature-id>/runtime/verification/evaluate-01/check-01.stderr.log`

If any verification command fails, the harness forces the feature QA status to `fail` regardless of the evaluator worker response.

If `[[evaluator.screenshots]]` is configured, the harness also captures screenshot evidence under:

- `features/<NN>-<feature-id>/runtime/screenshots/evaluate-01/report.json`
- `features/<NN>-<feature-id>/runtime/screenshots/evaluate-01/shot-01-<name>.png`
- `features/<NN>-<feature-id>/runtime/screenshots/evaluate-01/shot-01-<name>.stdout.log`
- `features/<NN>-<feature-id>/runtime/screenshots/evaluate-01/shot-01-<name>.stderr.log`

Screenshot commands receive a harness-owned output path through `{output}` replacement and the environment variables `CODEX_HARNESS_SCREENSHOT_OUTPUT`, `CODEX_HARNESS_SCREENSHOT_DIR`, `CODEX_HARNESS_SCREENSHOT_NAME`, and `CODEX_HARNESS_ATTEMPT`.

## Service supervision

When `runtime.supervision.enabled = true`, the harness starts all configured runtime stacks and services before build/evaluate work begins and stops them after the run finishes or fails.

Runtime stacks are command-driven orchestrators such as `docker compose up -d` / `down`. They are useful when the repo owns its runtime through containers or another external supervisor.

For each service it writes:

- `runtime/services/<service>/stdout.log`
- `runtime/services/<service>/stderr.log`
- `runtime/services/<service>/service.json`

For each stack it writes:

- `runtime/stacks/<stack>/up.stdout.log`
- `runtime/stacks/<stack>/up.stderr.log`
- `runtime/stacks/<stack>/down.stdout.log`
- `runtime/stacks/<stack>/down.stderr.log`
- `runtime/stacks/<stack>/stack.json`

Readiness can be driven by either:

- `ready_command`
- `ready_url`

If a supervised service fails to become ready, the run fails before feature execution continues.

## Validation

The current scaffold is covered by:

- unit tests for config resolution, controller state, workspace isolation, service supervision, and verification execution
- binary smoke tests for happy path, repair path, repair-budget exhaustion, multi-feature backlog execution, `inspect`, `resume`, deterministic verification failure gating, and git-worktree bootstrap paths before the first commit even when run artifacts live under the workspace
- live Codex E2E coverage for both the full run loop and planner-only routing when the local environment is Codex-ready

Run the user-journey suite with:

```bash
./scripts/run-user-journeys.sh
```

The runner always executes the deterministic journey lane first. It then auto-enables the live Codex lane when both of these are true:

- `codex` is installed on `PATH`
- `codex login status` succeeds

Use `CODEX_LIVE_E2E=1` to force the live lane on, or `CODEX_LIVE_E2E=0` to force skip mode.

Run the full suite with:

```bash
cargo test --workspace
```

For production-oriented validation, prefer `./scripts/run-user-journeys.sh`. The raw `cargo test --workspace` lane is still useful for direct Rust test execution, but the journey runner is the command that promotes into live Codex coverage automatically when the machine is ready for it.
