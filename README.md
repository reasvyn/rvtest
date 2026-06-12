<div align="center">
  <h1>rvtest</h1>
  <p><strong>A Next Level Testing Framework for Rust</strong></p>
</div>

<div align="center">

[![Crates.io][crates-badge]][crates-url]
[![GitHub][repo-badge]][repo-url]
[![MIT License][license-badge]][license-url]
[![Rust 1.96+][rust-badge]][rust-url]

</div>

[crates-badge]: https://img.shields.io/crates/v/rvtest.svg
[crates-url]: https://crates.io/crates/rvtest
[repo-badge]: https://img.shields.io/badge/github-reasvyn/rvtest-8da0cb?logo=github
[repo-url]: https://github.com/reasvyn/rvtest
[license-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[license-url]: https://github.com/reasvyn/rvtest/blob/main/LICENSE
[rust-badge]: https://img.shields.io/badge/rust-1.96%2B-blue?logo=rust
[rust-url]: https://www.rust-lang.org

---

`rvtest` extends Rust's built-in test harness with a rich suite of
features designed for real-world testing workflows — without
requiring any proc-macro magic or heavy dependencies.

## Features

- **BDD-style specs** – Organise tests with `describe`/`it` blocks,
  nested hierarchies, tags, per-suite timeouts, and retries.
- **Property-based testing** – Verify invariants over thousands of
  randomly generated inputs with automatic counterexample shrinking.
- **Parametrized tests** – Run the same test logic against multiple
  inputs without boilerplate.
- **Rich reporting** – Pretty (colourised), TAP, JUnit XML, JSON,
  and Compact formats.
- **Self-contained code coverage** – Measure line, function, and
  region coverage via `cargo rvtest --coverage`. Pure-Rust `.profraw`
  parser — no `llvm-profdata` or `llvm-cov` required.
- **Configurable runner** – Parallel execution, name & tag filtering,
  fail-fast, configurable timeouts, and retries for flaky tests.
- **No proc-macro dependencies** – Everything is plain Rust functions
  and closures. Works inside standard `#[test]` functions.

---

## Quick Start

Add `rvtest` to your `Cargo.toml`:

```toml
[dev-dependencies]
rvtest = "0.1.0"
```

### BDD-style specs

```rust
use rvtest::spec::describe;

#[test]
fn calculator_tests() {
    describe("Calculator")
        .it("adds two positive numbers", || {
            assert_eq!(2 + 2, 4);
        })
        .it("subtracts", || {
            assert_eq!(5 - 3, 2);
        })
        .tag("arithmetic")
        .timeout(std::time::Duration::from_secs(2))
        .run()
        .assert_all_pass();
}
```

Nested suites, lifecycle hooks, and retries are fully supported:

```rust
#[test]
fn database_tests() {
    describe("Database")
        .before_all(|| {
            // runs once before any child test
        })
        .after_all(|| {
            // runs once after all child tests
        })
        .describe("queries")
            .it("selects user by id", || { /* ... */ })
            .it("inserts new record", || { /* ... */ })
            .tag("smoke")
        .describe("transactions")
            .it("rolls back on error", || { /* ... */ })
            .retries(2)   // flaky test — retry twice
        .run()
        .assert_all_pass();
}
```

### Property-based testing

```rust
use rvtest::property::{check, any};

#[test]
fn addition_is_commutative() {
    check("commutativity", any::<i32>(), |a: &i32| {
        let b: i32 = 42;
        a + b == b + *a
    });
}
```

When a counter-example is found, `check` panics with the seed
and the (shrunk) minimal failing input.

### Parametrized tests

```rust
use rvtest::param::parametrize;

#[test]
fn addition_cases() {
    for case in parametrize("add", [(1, 1, 2), (0, 0, 0), (-1, 1, 0)], |(a, b, exp)| {
        assert_eq!(a + b, *exp);
    }) {
        assert!(case.status.is_passed(), "{} failed", case.name);
    }
}
```

---

## CLI Usage (`cargo rvtest`)

`rvtest` ships with a `cargo` subcommand that runs your project's
tests and renders the results in any supported format.

```bash
# Run all tests with the pretty reporter (default)
cargo rvtest

# Run only tests matching a name filter
cargo rvtest --filter arithmetic

# Run with verbose output (show passing tests too)
cargo rvtest -v

# Output in machine-readable formats
cargo rvtest -F json
cargo rvtest -F tap
cargo rvtest -F junit
cargo rvtest -F compact

# Collect code coverage (pure-Rust, no external tools needed)
cargo rvtest --coverage

# Coverage with different output formats
cargo rvtest --coverage --coverage-format json --coverage-dir ./coverage

# Tag-based filtering
cargo rvtest --tag smoke
cargo rvtest --tag arithmetic --exclude-tag slow

# Fail-fast mode
cargo rvtest --fail-fast
```

All options:

| Flag | Description |
|---|---|
| `-f, --filter` | Filter test names by substring (case-insensitive) |
| `-t, --tag` | Only run tests carrying all of these tags |
| `-E, --exclude-tag` | Skip tests carrying any of these tags |
| `-r, --retries` | Number of retries for flaky tests (default: 0) |
| `--timeout` | Default per-test timeout in seconds |
| `--no-parallel` | Run tests sequentially |
| `--fail-fast` | Stop after the first failure |
| `-F, --format` | Output format: `pretty`, `tap`, `junit`, `json`, `compact` |
| `-v, --verbose` | Show all tests (including passing ones) |
| `--coverage` | Enable code coverage collection |
| `--coverage-format` | Coverage format: `summary`, `html`, `lcov`, `json`, `cobertura` |
| `--coverage-dir` | Output directory for coverage artifacts |

---

## Reporting Formats

Five output formats are supported:

| Format | Description |
|---|---|
| **Pretty** (default) | Human-readable, colourised output with ✓/✗/– badges and timing |
| **TAP** | [Test Anything Protocol](https://testanything.org/) — line-based format widely supported by CI |
| **JUnit XML** | XML format understood by Jenkins, GitLab CI, GitHub Actions |
| **JSON** | Structured JSON output for programmatic consumption |
| **Compact** | Single-line-per-test summary for quick feedback |

---

## Code Coverage

`rvtest` includes a **self-contained coverage system** that works
without any external tools. It compiles your tests with LLVM
coverage instrumentation (`-Cinstrument-coverage`) and parses the
resulting `.profraw` files entirely in Rust — no `llvm-profdata`,
`llvm-cov`, or `cargo-llvm-cov` required.

```
$ cargo rvtest --coverage
Coverage: 48.7% lines, 56.1% functions, 48.7% regions
```

The coverage output is 100 % compatible with the format produced
by `llvm-cov report --summary-only`, so it can be used with any
tooling that understands LLVM coverage data.

If `cargo-llvm-cov` or `llvm-profdata`/`llvm-cov` are installed,
`rvtest` uses them automatically for enhanced report generation.

---

## How It Works

`rvtest` is designed as a **library** that you use inside standard
`#[test]` functions. The `describe`/`it` builder constructs a test
spec, and `run()` executes it — catching panics, measuring timing,
and recording results. `assert_all_pass()` panics with a detailed
report if any test failed, which causes the `#[test]` to fail
naturally.

The `cargo rvtest` CLI runs your project's tests via `cargo test`,
parses the output, and re-renders it using rvtest's reporting system.
This gives you all the format flexibility without requiring any
changes to your test code.

---

## Roadmap / Future Ideas

- **Proc-macro API** — `#[describe]` and `#[it]` proc macros for
  even cleaner test definitions.
- **Snapshot testing** — File-based snapshot assertions with
  automatic review workflow.
- **Flaky test detection** — Statistical flakiness analysis across
  multiple runs.
- **Slow test profiling** — Automatic identification of the slowest
  tests.
- **Watch mode** — Re-run tests on file changes.
- **GitHub Actions reporter** — Annotations for inline PR failure
  display.
- **Fuzzing integration** — Property-based test shrinking with
  coverage-guided fuzzing.

---

## License

`rvtest` is released under the MIT License.
