# rvtest Roadmap

> **A Next Level Testing Framework for Rust**

This document outlines the planned features, improvements, and
architectural direction for `rvtest`.  It is a living document —
priorities shift as the project evolves and community feedback
arrives.

---

## Current State (v0.1.0)

- BDD-style specs (`describe` / `it`, nesting, hooks, tags,
  timeouts, retries)
- Property-based testing (`check`, `Strategy` trait, `any`, vec/map/filter combinators, shrinking)
- Parametrized tests (`parametrize`, `parametrize_named`)
- Reporting: Pretty (colourised), TAP, JUnit XML, JSON, Compact
- Code coverage: self-contained pure-Rust `.profraw` parser;
  also supports `cargo-llvm-cov` and `llvm-tools` as backends
- CLI (`cargo rvtest`) with filter, tag, format, coverage options
- Tag and name filtering
- Parallel and sequential execution
- Retries and timeouts
- Source-location tracking (`file!` / `line!` / `column!`)
- Dogfooded integration tests

---

## Planned Features

### 0.x releases

#### 1. Proc-Macro API `#[describe]` / `#[it]`

**Goal:** Eliminate the `#[test]` wrapper and the builder boilerplate.
Provide a proc-macro that auto-registers specs so `cargo rvtest` can
discover them directly without parsing `cargo test` output.

**Design sketch:**

```rust
use rvtest::prelude::*;

#[describe("Calculator")]
mod calculator {
    #[it("adds two positive numbers")]
    fn adds() {
        assert_eq!(2 + 2, 4);
    }

    #[it("subtracts")]
    #[tag("arithmetic")]
    #[timeout(2_000)]
    fn subtracts() {
        assert_eq!(5 - 3, 2);
    }

    #[describe("advanced")]
    #[tags("math", "advanced")]
    #[retries(2)]
    mod advanced {
        #[it("handles large numbers")]
        fn large() {
            assert_eq!(1_000_000 + 1_000_000, 2_000_000);
        }
    }
}
```

**Implementation approach:**

- A `#[describe]` proc-macro on `mod` items that generates a `#[test]`
  function calling a registration function.
- Registration functions push specs into a global registry
  (`std::sync::OnceLock` or `linkme` slice).
- `cargo rvtest` reads the registry via a shared library interface
  or by linking against a small shim.

**Why this matters:**

- No more `#[test] fn wrapper() { describe(...).run().assert_all_pass() }`
- Auto-discovery for `cargo rvtest` — no parsing of `cargo test` stdout
- Better IDE support (one `#[test]` per spec module instead of one
  per test)

**Status:** 🟢 Built on `main` — `rvtest-macros` crate with `#[describe]` / `#[it]` / `#[tag]` / `#[timeout]` / `#[retries]` / `#[before_all]` / `#[after_all]`.  Optional `macros` feature on `rvtest`.  Nested `#[describe]` blocks supported, non-macro items preserved.  Dogfooded tests pass.  Pending release as v0.2.0.

---

#### 2. Arch Tests

**Goal:** Declarative architecture-enforcement tests that verify
module dependencies, visibility constraints, and layering rules
directly inside `rvtest` specs — no external linters required.

**Design sketch:**

```rust
use rvtest::arch::*;

#[test]
fn architecture() {
    arch_check()
        .module("core").may_not_depend_on("coverage", "report")
        .module("spec").may_depend_on("core", "tag")
        .module("runner").may_depend_on("core", "report", "spec")
        .all_modules().must_not_have_cycles()
        .assert_all_pass();
}
```

**How it works:**

- Scans `src/` for `.rs` files, parses `mod` declarations and `use`
  statements using lightweight text analysis (no `syn` dependency).
- Builds a directed dependency graph between modules.
- Checks each declared rule against the actual graph.
- Reports any violation with the offending import path.

**Built-in rules:**

| Rule | Description |
|---|---|
| `may_depend_on(...)` | Module can only depend on listed peers |
| `may_not_depend_on(...)` | Module cannot depend on listed peers |
| `must_not_have_cycles()` | No circular dependencies anywhere |
| `public_api(doc_required)` | Public items must have doc comments |

**Integration:**

- `rvtest::arch` module with `ArchCheck` builder and `Rule` types.
- `arch_check()` → `.module(...)`.rule...`.assert_all_pass()`.
- Works inside `#[test]` or `#[describe]` blocks.
- Violations rendered in the same failure format as spec tests.

**Status:** 🟢 Built on `main`.  `rvtest::arch` module with `arch_check()` builder, `may_depend_on` / `may_not_depend_on` / `must_not_have_cycles` rules.  Scans `src/` for `.rs` files, builds dependency graph, checks constraints.  Dogfooded test enforces rvtest's own module layering.  Pending release.

---

#### 3. Snapshot Testing

**Goal:** File-based snapshot assertions with automatic review
and update workflow, similar to `insta` but natively integrated
into rvtest's runner and reporter.

**Design sketch:**

```rust
#[test]
fn json_output() {
    let data = serde_json::to_string_pretty(&my_struct()).unwrap();
    rvtest::snapshot::assert_snapshot!("json_output", &data);
}
```

Or, with the planned proc-macro API:

```rust
#[describe("API output")]
mod api {
    #[it("serializes user correctly")]
    #[snapshot]
    fn user_json() -> String {
        serde_json::to_string_pretty(&User::new()).unwrap()
    }
}
```

**Features:**

- First run creates `.snapshots/` directory with pending snapshots
- `cargo rvtest --review` enters review mode (diff per snapshot,
  accept / reject)
- `cargo rvtest --update-all` accepts all pending snapshots
- Inline snapshots (value embedded in source as a special comment)
- CI mode: snapshot mismatch → failure with diff in JUnit/JSON output

**Integration points:**

- `rvtest::snapshot` module with `assert_snapshot` function
- New CLI flags: `--review`, `--update-all`
- Reporter support: include snapshot diffs in failure output
- JUnit reporter: append snapshot diff as `<system-out>` CDATA

**Status:** 🟢 Built on `main`.  `rvtest::snapshot` module with `assert_snapshot()` / `assert_snapshot_in()`.  `--update-all` flag auto-accepts new/updated snapshots.  Mismatch detection with line-diff output.  Dogfooded tests pass.  Pending release.

---

#### 4. Watch Mode

**Goal:** Re-run tests automatically when source files change.

**Design:**

```bash
cargo rvtest --watch
```

**Implementation:**

- Use the `notify` crate to watch source directories
- Debounce file-change events (250 ms)
- Re-run tests on the changed file's corresponding spec
- Display a "watching..." status bar with last-run timestamp
- Keybindings: `r` to re-run, `q` to quit, `f` to filter

**Approach:**

```rust
// Pseudocode
fn watch_loop(config: RunnerConfig) {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = notify::recommended_watcher(tx).unwrap();
    watcher.watch("src/", RecursiveMode::Recursive).unwrap();
    watcher.watch("tests/", RecursiveMode::Recursive).unwrap();

    loop {
        match rx.recv() {
            Ok(event) => {
                // Debounce
                // Re-run tests
                // Clear terminal, print results
            }
            Err(_) => break,
        }
    }
}
```

**Status:** 🟢 Built on `main`.  `cargo rvtest --watch` re-runs tests when files in `src/` or `tests/` change.  Uses `notify` for file watching, 300ms debounce, press `q` to quit.  Pending release.

---

#### 5. GitHub Actions Annotations Reporter

**Goal:** Produce GitHub Actions-compatible error annotations
from test failures.  This makes `cargo rvtest` drop-in ready for
CI pipelines.

**Usage:**

```bash
cargo rvtest -F github
```

**Output format:**

```
::error file=src/calculator.rs,line=42,title=Calculator :: adds — assertion failed
```

**Implementation:**

- New `GithubReporter` implementing `TestReporter`
- For each `Failed` / `TimedOut` test case, emit `::error` or
  `::warning` annotations
- Include the source location (file, line) when available
- Fall back to `compact` reporter format for the summary

**Status:** Not started.

---

#### 5. Flaky Test Detection

**Goal:** Automatically identify flaky tests (tests that
non-deterministically pass and fail).

**Design:**

```bash
cargo rvtest --detect-flaky
```

**Approach:**

- Run the entire test suite N times (default 10) with different
  random seeds
- Record the pass/fail history per test
- Report: "Flaky tests detected: 2 — `network_timeout` (70%
  pass rate), `concurrent_write` (45% pass rate)"
- Optionally mark flaky tests with a `#[flaky]` attribute or tag

**Integration:**

- Flaky detection as a mode of the runner, not a separate tool
- Results appear in the summary section of every reporter format
- The `--retries` flag already handles retrying — flaky detection
  extends this idea to detection across runs

**Status:** Not started.

---

#### 6. Slow Test Profiling

**Goal:** Automatically surface the slowest tests in the suite.

**Design:**

```bash
cargo rvtest --profile-slow
```

**Output:**

```
  ⏱  Slowest tests
    1.  2.34s  Database :: insert_large_batch
    2.  1.12s  API :: full_integration_flow
    3.  0.89s  Renderer :: complex_svg_output
```

**Implementation:**

- Collect per-test duration data
- Sort and display the top N slowest (configurable, default 5)
- Include in all reporter formats as an optional section
- `--profile-slow=N` shows top N

**Status:** Not started.

---

#### 7. Comprehensive Assertion Macros

**Goal:** Provide a rich set of assertion macros with automatic
diff output, similar to `pretty_assertions` but integrated into
rvtest's failure reporting.

**Macros:**

```rust
rvtest::assert_eq!(actual, expected);         // with structural diff
rvtest::assert_ok!(result);                   // unwrap or fail
rvtest::assert_err!(result);                  // unwrap Err or fail
rvtest::assert_matches!(value, Pattern);      // pattern matching
rvtest::assert_delta!(value, expected, eps);  // float comparison
```

**Failure output:**

```
  ✗ FAIL  Calculator :: adds  [0.2ms]
         assertion failed: `(left == right)`
         left:  42
         right: 43
```

Or, for complex types, a coloured side-by-side diff.

**Implementation:**

- New `rvtest-assertions` crate (or module in the main crate)
- Use `similar` crate for text diffs on `Debug` output
- Macros capture file/line via `file!()` / `line!()` and
  integrate with rvtest's `SourceLocation`

**Status:** Not started.

---

#### 8. Output Capture Per Test

**Goal:** Capture stdout/stderr per test case and show it only
on failure, avoiding the noisy `--nocapture` problem.

**Design:**

```rust
#[test]
fn noisy_test() {
    // stdout is captured by default
    println!("this won't show unless the test fails");
    assert!(true);
}
```

**Implementation:**

- Use the `output_capture` crate or `std::io::set_output_capture`
  (nightly) to redirect stdout/stderr per test
- Store captured output in `TestCase` as `captured_output: String`
- Reporters show captured output only on failure (or with `-v`)

**CLI flags:**

- `--show-output` — always show output (like `--nocapture`)
- Default: show output only on failure

**Status:** Not started.

---

### Post-1.0 Ideas

| Feature | Description |
|---|---|
| **Fuzzing integration** | Combine property-based testing with coverage-guided fuzzing (`libfuzzer` / `cargo-fuzz`). |
| **Benchmark integration** | `#[bench]` inside `describe` blocks, automatic regression detection. |
| **Nested before/after hooks** | `before_each` / `after_each` at any nesting level. |
| **Test matrix** | Run the same spec across multiple configurations (Rust versions, feature flags). |
| **Custom reporters via plugin** | Load external reporter crates via `cargo rvtest --reporter my-crate`. |
| **TUI mode** | Interactive terminal UI with real-time results, filtering, and drill-down. |
| **Cargo nextest integration** | Generate nextest-compatible output or run via nextest for enhanced sandboxing. |
| **HTML report** | Standalone HTML report with search, filtering, and coverage overlay. |

---

## Non-Goals

- **Replacing Cargo's test harness entirely.** `rvtest` is designed
  to complement `#[test]`, not replace it. Users can adopt features
  incrementally.
- **Runtime reflection or code generation.** The proc-macro API
  will use standard Rust macros, not build scripts or compiler plugins.
- **Stable-only.** `rvtest` targets stable Rust (edition 2024 once
  stable). The `output_capture` feature may require nightly until
  stabilised.

---

## Contributing

Ideas, bug reports, and pull requests are welcome.  If you'd like
to work on any of the items above, open an issue first to discuss
the design.

---

## Legend

| Status | Meaning |
|---|---|
| 🔴 Not started | No work done yet. |
| 🟡 In design | API sketch exists, implementation pending. |
| 🟢 Built, unreleased | Working code on `main`, not yet on crates.io. |
| ✅ Stable | Shipped in the latest release. |
