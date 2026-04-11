# Iteration Criteria
1. **Preserve approved user journeys first.**  
Existing user journey tests and smoke tests are the highest-priority contract.  
- Do **not** modify, delete, weaken, or re-interpret any existing approved user journey path or smoke test without explicit user approval.
- You **may** add new user journey paths or smoke tests when they clarify requirements, cover regressions, or verify newly introduced behavior.
- Success requires that all approved user journey paths and smoke tests pass exactly as defined.

2. **Refactor only when it improves delivery or correctness.**  
You may refactor legacy code or design without preserving backward compatibility **inside the codebase**, as long as:
- all approved user journey tests and smoke tests still pass,
- the refactor does not silently change intended user-facing behavior,
- and the refactor reduces complexity, risk, or maintenance burden.
- Avoid opportunistic refactors that do not directly support the current task.

3. **Ship production-grade outcomes.**  
Build solutions suitable for real production use, not demos or placeholders.  
- Unless the user explicitly says otherwise, prefer:
   - maintainable structure,
   - clear error handling,
   - observable behavior,
   - testable design,
   - secure and reliable defaults,
   - and minimal hardcoded assumptions.

4. **Validate before declaring completion.**  
Run the relevant user journey tests, smoke tests, and any directly impacted checks before considering the task complete.
- Run fast/local checks during implementation when useful.
- Run the full relevant validation set at the end of implementation.
- Do not claim success unless the required validations pass, or you explicitly report what could not be validated.

5. **Record durable learnings.**  
When user feedback, runtime evidence, or repository facts contradict your assumptions or prior implementation:
- trace the root cause,
- append a new entry to the `Learnings` section in `AGENTS.MD`,
   - include a timestamp, context, root cause, corrective insight, and any resulting rule or guardrail,
   - and check whether the new learning contradicts earlier learnings. If it does, update or supersede the older learning explicitly rather than leaving both ambiguous.

6. **Turn learnings into regression protection when appropriate.**  
For every new Learning, evaluate whether a user journey test, smoke test, or other automated check should be added or updated to prevent recurrence.
   - Add or propose the smallest effective regression coverage.
   - Do not modify existing approved user journey paths without explicit user approval.

---

# 🤖 Rust Code Generation Rules

## Core Philosophy

Prefer explicitness, ownership clarity, and compile-time guarantees.

Mental model:

* Dynamic languages → runtime validation
* Rust → boundary validation + compile-time invariants

Goals:

* Make illegal states unrepresentable (when practical)
* Keep ownership and data flow explicit
* Prefer compile-time safety over runtime checks
* Generate idiomatic Rust, not translated OOP designs
* Prefer simple, concrete designs before abstraction

---

## Priority Order (CRITICAL)

When rules conflict, follow this order:

```text
1. Correctness
2. Clarity
3. Maintainability
4. Performance (unless explicitly required)
5. Abstraction
```

---

## Conflict Resolution Rules

When unsure:

* Prefer **simpler APIs** over complex lifetime signatures
* Prefer **concrete types** over traits
* Prefer **clarity** over cleverness
* Prefer **borrowing** unless it complicates API design
* Prefer **fewer concepts** over “perfect” architecture

---

## Scope Control (VERY IMPORTANT)

* Do not introduce new abstractions unless required
* Do not refactor unrelated code
* Do not redesign architecture unless explicitly asked
* Keep changes minimal, local, and reviewable

---

## Documentation

Document **public APIs and non-obvious logic only**.

Provide documentation for:

* public structs, enums, traits, and functions
* public modules
* invariants and constraints
* ownership-sensitive logic
* concurrency or unsafe logic

Rules:

* `///` → public APIs
* `//!` → module docs (when useful)
* `//` → internal reasoning

Good:

```rust
/// Parses a port number from user input.
/// Ensures the value is within valid TCP range (1–65535).
```

Bad:

```rust
// Parse string to number
```

---

## Design Preferences

Prefer:

* concrete types first
* small traits only when multiple implementations exist
* enums instead of string states
* newtypes for domain concepts
* explicit dependency injection
* simple APIs

Avoid:

* global mutable state
* unnecessary heap allocation
* unnecessary lifetime complexity
* premature abstraction

---

## Ownership & Data Flow

Rules:

* Prefer `&str` over `String`
* Prefer `&[T]` over `Vec<T>`
* Clone only when necessary
* Take ownership only when required

Good:

```rust
fn normalize(name: &str) -> String
```

Bad:

```rust
fn normalize(name: String) -> String
```

---

## Idiomatic Patterns (Use When Justified)

### Use traits when:

* there are **2+ real implementations**
* behavior needs to be interchangeable

### Use newtypes when:

* a primitive has domain meaning

```rust
struct UserId(u64);
```

### Use typestate when:

* states are strict and ordered
* invalid states can be eliminated at compile time

Do NOT use typestate for simple CRUD logic.

### Use builder when:

* many optional or configurable fields exist

---

## Iteration

Prefer clarity over style.

Good:

```rust
for user in users {
    if user.active {
        active.push(user.id);
    }
}
```

Also good:

```rust
users.iter().filter(|u| u.active).map(|u| u.id).collect()
```

Avoid overly complex iterator chains.

---

## Concurrency

Prefer:

* ownership transfer
* message passing
* `Arc<T>` for shared immutable state

Avoid by default:

* `Arc<Mutex<T>>`
* global shared mutable state

Use shared mutability only when required.

---

## Error Handling

Prefer:

* `Result<T, E>` → fallible operations
* `Option<T>` → absence
* typed errors

Use:

* `thiserror` → libraries
* `anyhow` → applications only

Good:

```rust
enum Status {
    Active,
    Inactive,
}
```

Bad:

```rust
fn set_status(status: &str)
```

Do not collapse errors into strings prematurely.

---

## Testing

Generated code should include tests when behavior is non-trivial.

Prefer:

* unit tests for logic
* regression tests for bugs
* edge case coverage

---

## Async

* Use async only when needed
* Do not block in async code
* Use async-compatible libraries
* Avoid unbounded task spawning

---

## Unsafe Code

* Avoid `unsafe` unless required
* If used:

  * minimize scope
  * document safety invariants
  * wrap in safe abstractions

---

## Hard Rules

Never:

* use `unwrap()` in library/core logic
* use stringly-typed states when enums apply
* introduce `Rc<RefCell<T>>` casually
* rely on global mutable singletons

Avoid exposing unnecessary lifetime complexity in public APIs.

---

## Code Expectations

Generated code must:

* be idiomatic Rust
* have clear ownership semantics
* encode invariants in types where useful
* include meaningful (not noisy) documentation
* prioritize correctness and clarity
* avoid unnecessary abstraction

---

## Agent Behavior Rules

When generating or modifying code:

* Read surrounding code first
* Match existing project conventions
* Keep diffs minimal and focused
* Ensure imports and visibility are correct
* Avoid unnecessary clones or allocations
* Add tests when behavior changes

When unsure:

→ choose the **simpler solution**

---

# Learnings

- 2026-04-09: Discovery layering must remain evidence-backed.
  Context: The project settings pane showed generic layer rules like "UI may depend inward on service and core" for this workspace, and the user correctly called them out as fake.
  Root cause: `build_layering_profile` promoted path-name heuristics (`ui`, `core`, `adapter`) into hardcoded architectural dependency rules and canned responsibilities, even when the scan had no explicit architecture evidence.
  Corrective insight: Heuristic layer-name detection can suggest possible layers, but it cannot justify dependency-direction rules or responsibilities on its own.
  Guardrail: Discovery should emit heuristic layer names as ambiguous unless explicit dependency or architecture evidence supports stronger claims, and regression tests should fail if generic layer rules reappear from naming heuristics alone.

- 2026-04-09: Discovery facts must carry negentropy tier metadata.
  Context: All discovery facts were stored as flat `DiscoveryFact` with identical weight. A coding convention file (`.editorconfig`) and an auth keyword match (`token` in `tokenizer.rs`) received the same treatment, losing information about their relative constraining power.
  Root cause: The original design treated discovery as "file classification" rather than "knowledge extraction". `DiscoveryFact` lacked a dimension to express information value, and `synthesize_profile_from_evidence` simply cloned scan data without compression.
  Corrective insight: Discovery output must distinguish four negentropy tiers (Specification > Verification > Structure > Implementation). Higher-tier facts should receive deeper content extraction, priority ordering in prompts, and consolidation during profile synthesis. Detection functions must use precise matching (path-segment matching, not substring; strong keywords vs. contextual keywords with auth-adjacent patterns) to avoid false positives that pollute higher tiers.
  Guardrail: Regression tests verify that (1) layer candidates match exact path segments, not substrings, (2) ambiguous keywords like "token" alone do not trigger auth detection, (3) convention files receive Specification tier with full content for small files, (4) duplicate tech_stack entries are consolidated, and (5) prompt_context outputs specifications before structural data.

- 2026-04-09: Discovery scan must cover three missing negentropy layers: project intent, environment requirements, and change boundaries.
  Context: WorkspaceProfile had no field representing "what this project is for" (L0 goal layer), no field representing "what runtime environment is required" (L3 runtime constraints), and no field representing "what files should not be changed" (change boundaries). This left downstream consumers (planner, builder, evaluator) without anchoring context for decisions.
  Root cause: The original scan focused exclusively on code-level facts (tech_stack, api_contracts, auth) but missed three categories of high-negentropy data that are reliably extractable from deterministic files: README descriptions, toolchain/version pinning files, and lock/CI/deploy files.
  Corrective insight: (1) `project_intent` should extract from README.md, Cargo.toml `description`, and package.json `description` — these are L0/Specification tier with very low false-positive risk. (2) `environment_requirements` should extract from rust-toolchain.toml, .nvmrc, .tool-versions, .env.example, docker-compose.yml — these are L3/Structure tier. (3) `change_boundaries` should classify lock files as frozen_paths and CI/deploy configs as high_risk_paths, injected into both prompt_context and contract_scope_notes as hard constraints.
  Guardrail: Regression tests verify that (1) README.md is detected as project_intent with Specification tier, (2) Cargo.toml description is extracted, (3) Cargo.lock is classified as frozen, (4) rust-toolchain.toml is detected as environment requirement, and (5) prompt_context outputs project_intent before key_concepts and includes frozen_files/high_risk_files sections.
