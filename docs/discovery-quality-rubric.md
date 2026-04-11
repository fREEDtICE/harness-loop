# Discovery Quality Rubric

This rubric evaluates discovery as a negentropy layer for the harness.

The goal is not to reward verbose summaries. The goal is to verify that
discovery preserves the highest-governance facts per token and prevents
lower-level implementation details from silently redefining higher-level
constraints.

## Core Principle

Discovery quality is the ability to compress repository truth into a
planner-safe control model without inverting the source-of-truth hierarchy.

Hierarchy:

- Goals > Effects > Processes
- Specifications > E2E Tests > Code

Lower-negentropy data may refine higher-negentropy data, but it must not
override it.

## Evaluation Questions

1. Is the evidence correct and traceable?
2. Are the inferences justified by that evidence?
3. Does the final profile faithfully preserve the governing constraints?
4. Are the saved artifacts fresh and safe to inject into planner/build prompts?

## Dimensions

### 1. Source Fidelity

Checks:

- every fact has provenance
- evidence items remain tied to real source paths or manifests
- duplicates are suppressed before they waste token budget
- stale or partial artifacts are not presented as current truth

Failure examples:

- facts with no evidence path
- commands with no source file
- stale profile presented after a newer scan

### 2. Governance Fidelity

Checks:

- higher-tier facts remain governing
- heuristic observations do not become policy
- profile claims do not outrun evidence or inference
- missing goal/spec/test signals are surfaced as risk

Failure examples:

- path-name heuristics turned into architecture rules
- code-level observations overriding explicit tests or specs
- profile claims present without supporting evidence chains

### 3. Compression Faithfulness

Checks:

- the saved profile remains a faithful projection of evidence + inference
- duplication is removed before summarization
- key concepts remain traceable to governing artifacts

Failure examples:

- stored profile differs from deterministic assembly
- repeated stack names or commands dominate the profile
- profile summary introduces untraceable claims

### 4. Operational Readiness

Checks:

- discovery reached a terminal `ready` state
- fallback profiles are not silently treated as current
- inference exists and is structurally valid
- prompt-injected context is fresh enough to govern downstream work

Failure examples:

- discovery stuck in `polishing`
- fallback profile used after the workspace changed
- missing inference artifact

## Confidence Standard

Confidence belongs to inference, not evidence.

- `10`: only allowed with exactly one strong evidence chain
- `8-9`: strong support with small interpretation
- `5-7`: plausible synthesis
- `3-4`: weak or heuristic
- `1-2`: tentative

High confidence must be capped by source strength and source tier. Heuristics
must not produce high-confidence governance claims.

## Quality Gate

The `discover-eval` report produces:

- `pass`
- `warn`
- `fail`

The gate fails when discovery is stale, incomplete, structurally invalid, or
contains governance inversions such as unsupported profile claims or heuristic
layer rules presented as truth.

## CI Benchmark Usage

Recommended CI sequence:

1. `loopsmith discover --config ... --workspace ...`
2. `loopsmith discover-eval --workspace ...`

`discover-eval` writes `.loopsmith/discovery/quality-report.json` and exits
non-zero when the quality gate fails. That makes it suitable both for human
inspection and automated regression protection.
