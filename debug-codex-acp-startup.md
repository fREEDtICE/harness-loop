[OPEN] codex-acp-startup

# Debug Session: codex-acp-startup

## Symptom
- `loopsmith discover` can spawn the configured `codex` binary, but ACP initialization fails with: `Internal error: "server shut down unexpectedly"`.

## Expected
- The local `codex` binary should remain alive long enough to complete ACP `initialize`, then serve discovery prompts.

## Hypotheses
1. The configured `codex` binary does not support ACP server mode when invoked without extra subcommands or flags.
2. The configured `codex` binary supports ACP, but requires a dedicated subcommand or startup flag that LoopSmith is not passing.
3. The `codex` process exits early because its runtime environment is incomplete when launched from LoopSmith.
4. The `codex` binary is the wrong executable/version for ACP, even though the path itself is valid.
5. The `codex` binary starts, but immediately rejects stdio JSON-RPC/ACP handshake input and terminates.

## Evidence Plan
- Inspect the installed `codex` binary version/help output.
- Probe likely ACP startup forms directly from the terminal.
- Reproduce the process behavior outside LoopSmith to compare exit mode and stderr.

## Status
- Session opened. No business logic changed.

## Evidence
- `codex --version` reports `codex-cli 0.117.0`.
- `codex --help` exposes `mcp-server` as the only explicit stdio server mode.
- Running bare `codex` under non-TTY stdio exits immediately with: `TERM is set to "dumb"... Refusing to start the interactive TUI`.
- Running `codex mcp-server` under stdio stays alive without exiting, which is consistent with server-mode behavior.
- LoopSmith's worker client currently uses `agent_client_protocol` and sends ACP `initialize` via `ClientSideConnection`, not MCP stdio.
- `npx -y @zed-industries/codex-acp --help` succeeds and identifies the package as `codex-acp`.
- Launching `npx -y @zed-industries/codex-acp` under stdio remains alive until explicitly terminated, which is consistent with an ACP agent entrypoint.

## Hypothesis Status
- H1 confirmed: bare `codex` is not a usable ACP server entrypoint.
- H2 partially confirmed: the CLI does require a dedicated server subcommand, but the available one is `mcp-server`.
- H3 not supported by current evidence: failure reproduces outside LoopSmith due to startup mode mismatch, not missing env.
- H4 not supported by current evidence: binary exists and responds normally.
- H5 plausible as protocol mismatch, but weaker than H1/H2 because the dominant failure happens before any valid ACP server mode is selected.

## Interim Conclusion
- The installed `codex` CLI can run as an MCP stdio server, but LoopSmith is currently launching it as if it were an ACP stdio server.
- Bare `codex` falls into its interactive CLI/TUI mode rather than a stdio server mode; it is not launching the desktop app in this path.
- The observed `ACP initialize failed: Internal error: "server shut down unexpectedly"` is consistent with this startup/protocol mismatch.
- `codex-acp` is a more plausible execution target for LoopSmith's current ACP client than the bare `codex` binary.

## Fix Applied
- Switched the workspace ACP command to `npx -y @zed-industries/codex-acp`.
- Upgraded `agent-client-protocol` from `0.4` to `0.10.4` and enabled the `unstable` feature so `usage_update` session notifications can be decoded.
- Updated `loopsmith-acp` client code to the newer ACP Rust SDK builder API and tuple enum variants.
- Added a regression test that deserializes a `session/update` notification carrying `usage_update`.

## Pre-Fix vs Post-Fix
- Pre-fix runtime evidence: `discover` completed, but logged `unknown variant usage_update` while decoding a session notification from `codex-acp`.
- Post-fix runtime evidence: `discover` reaches `phase: ready`, `used_fallback_profile: false`, and no longer logs the `usage_update` decode error.
- Post-fix validation:
  - `cargo check -p loopsmith-acp` ✅
  - `cargo test -p loopsmith-acp` ✅
  - `cargo run -- discover --config .loopsmith/config.toml --workspace /Users/bytedance/Documents/Dev/codex-harness-rs` ✅

## Remaining Observation
- The ACP worker still logs a cleanup warning after `discover`: `failed to clean up ACP agent descendant processes`. This did not block discovery success, but it is a remaining process-lifecycle issue worth addressing separately if we want a clean integration.
