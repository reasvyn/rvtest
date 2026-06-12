# rvtest Philosophy

> Design principles behind **A Next Level Testing Framework for Rust**

---

## Principles

### 1. Complements `#[test]`, Does Not Replace It

`rvtest` is designed to work *inside* standard Rust `#[test]`
functions, not to replace the entire test harness.  Users can adopt
individual features (BDD specs, property checks, parametrized cases)
without changing their project structure or CI pipeline.

The CLI (`cargo rvtest`) is a value-add on top — it runs `cargo test`
under the hood and re-renders the output.  If you prefer the default
test harness, `rvtest` the library still works perfectly.

### 2. Zero Proc-Macros by Default

The core API uses only plain Rust functions, closures, and builder
patterns.  No proc-macro dependencies in the base crate means:

- Faster compilation
- No `syn`/`quote`/`proc-macro2` bloat for users who only need the
  library API
- Works on stable Rust without any feature gates

A proc-macro crate (`rvtest-macros`) may be added later as an
optional add-on for enhanced DX (`#[describe]` / `#[it]`), but the
core experience must always work without it.

### 3. Explicit Over Magic

Tests should be straightforward to understand.  The `describe` / `it`
builder chain is explicit about what it does:

```rust
describe("Calculator")
    .it("adds", || assert_eq!(2 + 2, 4))
    .run()
    .assert_all_pass();
```

There is no global state, no hidden registration, and no trait
machinery required.  Every step is visible in the code.

### 4. Composable Building Blocks

Features compose naturally:

- `property::check()` can be called inside `spec::Spec::it()`
- `param::parametrize()` can wrap any closure, including those that
  call `check()`
- `runner::TestRunner` accepts any number of specs
- Reporters work on `TestRun`, regardless of how it was produced

This composability means you can mix BDD specs, property tests, and
parametrized cases in any combination within a single test function.

### 5. Easy to Adopt, Easy to Remove

Adding `rvtest` to a project should be a single `cargo add` away.
Removing it should be as simple — the library dependency is
dev-only, and standard `#[test]` functions require no rvtest
imports at all.

### 6. Meaningful Failure Messages

When a test fails, the output should tell you *what* failed,
*where* it failed, and *why*.  The `assert_all_pass()` panic
includes the suite name, test name, duration, and failure reason.
Source locations are captured via `file!()` / `line!()` so you
can jump directly to the failing test.

Coverage output matches the `llvm-cov` format exactly — tools and
scripts that parse LLVM coverage data work with rvtest output
without modification.

### 7. Dogfooding

`rvtest`'s own integration tests are written using `rvtest`'s
BDD API.  This ensures the API is ergonomic in practice and that
any breaking changes are caught during development.

---

## What rvtest Is Not

- **Not a test harness replacement.**  Cargo's `#[test]` discovery
  and execution is still the foundation.
- **Not a mocking framework.**  Mocking/stubbing is outside scope;
  use `mockall`, `mockito`, or similar alongside rvtest.
- **Not a benchmarking tool.**  Simple timing is provided per-test,
  but microbenchmarking is a future concern.
- **Not a fuzzer.**  Property-based testing shares DNA with fuzzing,
  but coverage-guided fuzzing is a separate tool (see roadmap).
