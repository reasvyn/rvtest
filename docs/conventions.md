# rvtest Conventions

> Code conventions and patterns used throughout the rvtest codebase.

---

## Rust Edition and Toolchain

- **Edition:** 2024
- **Minimum Rust:** 1.96+
- **Format:** `rustfmt` with default settings
- **Lints:** `#![warn(unused)]` active; zero warnings expected

---

## Naming

| Category | Convention | Example |
|---|---|---|
| Crates | Lowercase, hyphenated | `rvtest`, `rvtest-macros` |
| Types | `PascalCase` | `TestSuite`, `RunnerConfig` |
| Functions | `snake_case` | `describe`, `assert_all_pass` |
| Methods on builder types | Fluent, consume `self` | `.it()`, `.tag()`, `.run()` |
| Error messages | Sentence case, no trailing period | `test panicked` |
| Doc comments | Sentence case, trailing period | `/// Create a new empty suite.` |

---

## Module Structure

```
src/
  lib.rs          -- Public API, prelude, module declarations
  core.rs         -- Core types (TestSuite, TestCase, TestStatus, etc.)
  spec.rs         -- BDD spec builder (describe / it)
  runner.rs       -- TestRunner, execution, run_tests helpers
  report.rs       -- TestReporter trait and all format implementations
  param.rs        -- Parametrized tests
  property.rs     -- Property-based testing (Strategy, check)
  tag.rs          -- Tag and name filtering
  coverage.rs     -- Coverage collector with multi-strategy fallback
  coverage_raw.rs -- Pure-Rust .profraw parser
  main.rs         -- CLI entry point (cargo-rvtest binary)
```

---

## Error Handling

- Functions return `Result<T, String>` for fallible operations.
  The `String` error message is user-facing and should be descriptive.
- For internal invariants that should never fail, use `expect()` with
  a message explaining *why* it should never fail.
- Panic messages from test closures are caught by `catch_unwind` and
  stored as `TestStatus::Failed { reason, .. }`.

---

## Reporters

Every reporter implements the `TestReporter` trait:

```rust
pub trait TestReporter {
    fn report(&self, run: &TestRun) -> String;
}
```

- **PrettyReporter** — default; colourised, human-readable.
  Hides passing tests unless `verbose` is true.
- **TapReporter** — TAP protocol; must produce valid `1..N` output.
- **JunitReporter** — JUnit XML; `<testsuites>` wrapping `<testsuite>`
  per suite, `<testcase>` per test.
- **JsonReporter** — single JSON object with `suites` array.
- **CompactReporter** — one line per test; always shows all tests.

When adding a new reporter:
1. Implement `TestReporter` in `report.rs`
2. Add a variant to `ReportFormat` in `core.rs`
3. Add `FromStr` parsing for the format name
4. Register in `render()` in both `main.rs` and `runner.rs`

---

## Coverage Strategies

The `CoverageCollector` tries strategies in order:

1. `cargo-llvm-cov` — best output quality
2. `llvm-tools` (manual `llvm-profdata` + `llvm-cov`)
3. **Self-contained `.profraw` parser** (default for fresh installs;
   pure Rust, no external deps)
4. Built-in sampler (`ptrace` + `addr2line`, Linux only)

Each strategy returns a `CoverageReport` with the same fields:

```
CoverageReport {
    line_coverage: f64,      // 0.0 – 100.0
    function_coverage: f64,  // 0.0 – 100.0
    region_coverage: f64,    // 0.0 – 100.0
    format: CoverageFormat,
    report_path: Option<PathBuf>,
}
```

---

## Testing

- Integration tests live in `tests/integration.rs` and use `rvtest`'s
  own BDD API (dogfooding).
- Each feature area has its own `#[test]` function with `describe()`
  blocks covering sub-features.
- Tests that verify failure cases use `catch_unwind` to assert
  that `assert_all_pass()` panics as expected.
- Reporter tests construct a `TestRun` directly and assert on the
  rendered string output.

---

## CLI Flags

CLI flags follow conventions similar to Rust's built-in tools and `clap`:

| Style | Example |
|---|---|
| Short flags | `-v`, `-f`, `-F` |
| Long flags | `--verbose`, `--filter`, `--format` |
| Short + value | `-f test_name` |
| Long + value | `--filter test_name` |
| Repeatable | `-t smoke -t slow` |
| Boolean | `--no-parallel`, `--fail-fast` |
