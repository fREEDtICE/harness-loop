# Iteration Criteria
1. **User journey paths have the highest priority.** Any modification to existing user journey tests MUST be approved by the user (me). You may add new user journey paths. The acceptance criteria is that all tests fully match the defined user journeys (including all paths).
2. **Refactor legacy code/designs at any time** (without backward compatibility), provided the refactored implementation passes all user journey tests.
3. **Build real production-grade applications**, not toys or demos.
4. **Run the User Journey tests** to ensure all test cases **once all code implementation is complete**

---

# 🤖 Rust Code Generation Rules (AI-Optimized)

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

