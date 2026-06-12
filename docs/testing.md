# Testing Policy — Dogfooding

> **rvtest tests itself with rvtest.**  All integration tests MUST use
> the `describe` / `it` BDD API.  Raw `#[test]` functions are reserved
> for bootstrapping and doc-tests only.

---

## Why Dogfooding

1. **Quality signal** — If the API is awkward to use in our own tests,
   it will be awkward for users.  Dogfooding forces us to eat our own
   dog food.

2. **Regression detection** — Breaking changes in the builder API or
   runner are caught immediately because our tests depend on the exact
   same chain: `describe().it().run().assert_all_pass()`.

3. **Living documentation** — Our test suite serves as a canonical set
   of usage examples.  New contributors can look at the tests to
   understand how features work end-to-end.

4. **Coverage of edge cases** — Retries, timeouts, hooks, nesting,
   tags, parametrized tests, property checks — all must be exercised
   in the dogfooded suite.

---

## Rules

### Rule 1: Integration tests go through `describe` / `it`

Every integration test in `tests/integration.rs` MUST be structured
as a `#[test]` function that calls `describe()` → `.it()` → `.run()`
→ `.assert_all_pass()`.

```rust
// ✅ CORRECT — dogfooded
#[test]
fn rvtest_spec() {
    describe("Spec")
        .it("passes when all tests pass", || {
            describe("Math")
                .it("adds", || assert_eq!(2 + 2, 4))
                .run()
                .assert_all_pass();
        })
        .run()
        .assert_all_pass();
}
```

```rust
// ❌ WRONG — bare #[test] with no describe
#[test]
fn add() {
    assert_eq!(2 + 2, 4);
}
```

### Rule 2: Every API must be dogfooded

When adding a new public API, a corresponding dogfooded test MUST be
added in the same PR.  The test MUST exercise the API through
`describe()` / `it()` rather than calling the API directly.

### Rule 3: `#[should_panic]` is allowed only at the outer level

Tests that verify failure behaviour (e.g. `assert_all_pass` panics)
may use `#[should_panic]` on the outer `#[test]` function, or use
`catch_unwind` inside a dogfooded `.it()` block.

```rust
// ✅ CORRECT — catch_unwind inside describe/it
#[test]
fn rvtest_reporters() {
    describe("Reporters")
        .it("pretty reporter shows summary", || {
            let result = std::panic::catch_unwind(|| {
                describe("Failing")
                    .it("fails", || panic!("intentional"))
                    .run()
                    .assert_all_pass();
            });
            assert!(result.is_err());
        })
        .run()
        .assert_all_pass();
}
```

### Rule 4: Test function names use the `rvtest_` prefix

All test functions that exercise rvtest itself MUST be named with the
`rvtest_` prefix for easy discovery:

```
rvtest_spec
rvtest_parametrized
rvtest_property
rvtest_runner
rvtest_reporters
rvtest_architecture
rvtest_snapshot_create_and_match
rvtest_snapshot_mismatch_detected
```

### Rule 5: Keep tests independent

Each `#[test]` function is a separate `describe` block that covers one
feature area.  Tests must not share mutable state — use local
`AtomicU32` or fresh `TestRun` instances inside each `.it()` closure.

---

## Enforcement

- **CI:** `cargo test` MUST pass with zero failures and zero warnings.
- **Code review:** Every PR is checked for dogfooding compliance.
- **Exceptions:** Only the `rvtest-macros` proc-macro crate may have
  integration tests that import macros directly (`use rvtest_macros::*`)
  instead of going through the `rvtest` re-export.  These tests still
  use `#[describe]` / `#[it]` macros, which are the proc-macro form
  of the same API.

---

## Current Coverage

Every public feature listed below has at least one dogfooded test:

| Feature | Test(s) |
|---|---|
| Basic specs | `rvtest_spec` |
| Nesting | `rvtest_spec` |
| Tags | `rvtest_spec`, `rvtest_parametrized`, `rvtest_property`, `rvtest_runner`, `rvtest_reporters` |
| Timeouts | `rvtest_spec` |
| Retries | `rvtest_spec` |
| Before/after hooks | `rvtest_spec` |
| Parametrized tests | `rvtest_parametrized` |
| Property-based tests | `rvtest_property` |
| Runner config | `rvtest_runner` |
| Reporters (all 5) | `rvtest_reporters` |
| Architecture tests | `rvtest_architecture` |
| Snapshots | `rvtest_snapshot_create_and_match`, `rvtest_snapshot_mismatch_detected` |
| Proc-macros | `rvtest-macros/tests/integration.rs` (5 tests) |
