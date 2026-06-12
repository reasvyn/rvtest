# rvtest Architecture

> Internal architecture of the rvtest testing framework.

---

## High-Level Overview

```
┌────────────────────────────────────────────────────────────────┐
│                        cargo rvtest (CLI)                      │
│  ┌─────────┐  ┌──────────┐  ┌───────────┐  ┌────────────────┐ │
│  │  Parser  │  │  Runner  │  │ Coverage  │  │   Reporters    │ │
│  │ (clap)   │  │ (cargo   │  │Collector  │  │ Pretty / TAP   │ │
│  │          │  │  test)   │  │ (profraw) │  │ JUnit / JSON   │ │
│  └─────────┘  └──────────┘  └───────────┘  │ / Compact      │ │
│                                             └────────────────┘ │
└────────────────────────────────────────────────────────────────┘
                           │ uses
                           ▼
┌────────────────────────────────────────────────────────────────┐
│                     rvtest (library crate)                      │
│                                                                 │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌───────────────┐ │
│  │  spec    │  │  property│  │   param   │  │    tag        │ │
│  │describe/ │  │ Strategy │  │parametrize│  │ tag/name      │ │
│  │ it/run   │  │ check    │  │           │  │ filtering     │ │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  └───────┬───────┘ │
│       │             │              │                 │         │
│       └─────────────┴──────────────┴─────────────────┘         │
│                               │ produces                       │
│                               ▼                                │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌───────────────┐ │
│  │  core    │  │  runner  │  │  report   │  │   coverage    │ │
│  │TestSuite │  │TestRunner│  │TestReporter│  │ Collector +   │ │
│  │TestCase  │  │ run_tests│  │ Pretty/    │  │ RawParser     │ │
│  │TestStatus│  │          │  │ Json/...   │  │               │ │
│  └──────────┘  └──────────┘  └───────────┘  └───────────────┘ │
└────────────────────────────────────────────────────────────────┘
```

---

## Core Data Flow

```
User code                         rvtest library
──────────                        ──────────────

describe("Math")                  Spec { name, children, tests }
  .it("adds", || ...)            ──► push TestEntry { name, fn }
  .it("subs", || ...)
  .run()                          collect_tests()
      │                               │
      │                           execute_test()
      │                               │
      │                           catch_unwind(test_fn)
      │                               │
      │                           TestCase { status, duration }
      │                               │
      ▼                               ▼
  TestSuite { tests }             TestSuite
  .assert_all_pass()              panic! on any failure
```

```
CLI flow
────────

cargo rvtest
    │
    ├─►── cargo test (subprocess)
    │       │
    │       ▼
    │   parse_cargo_test_output()
    │       │
    │       ▼
    │   TestRun { suites, duration }
    │       │
    │       ▼
    │   reporter.report(&run)
    │       │
    │       ▼
    │   stdout
    │
    └─►── --coverage
            │
            ├─► cargo test --no-run (with -Cinstrument-coverage)
            │       │
            │       ▼
            │   run test binaries
            │       │
            │       ▼
            │   collect .profraw files
            │       │
            │       ▼
            │   parse_raw_profile()
            │       │
            │       ▼
            │   CoverageReport
            │
            └─► (or: cargo-llvm-cov / llvm-tools)
```

---

## Key Types

### `TestRun` (aggregate root)

```rust
pub struct TestRun {
    pub suites: Vec<TestSuite>,
    pub start_time: SystemTime,
    pub end_time: SystemTime,
    pub duration: Duration,
}
```

### `TestSuite` (one describe block or binary output)

```rust
pub struct TestSuite {
    pub name: String,
    pub description: Option<String>,
    pub tests: Vec<TestCase>,
    pub duration: Duration,
}
```

### `TestCase` (single test execution result)

```rust
pub struct TestCase {
    pub name: String,
    pub suite: Option<String>,
    pub tags: Vec<String>,
    pub status: TestStatus,
    pub duration: Duration,
    pub assertions: u64,
    pub location: Option<SourceLocation>,
    pub parameters: Vec<(String, String)>,
}
```

### `TestStatus` (outcome of one test)

```rust
pub enum TestStatus {
    Passed,
    Failed { reason: String, location: Option<SourceLocation> },
    Skipped { reason: Option<String> },
    TimedOut { duration: Duration, location: Option<SourceLocation> },
}
```

---

## Module Dependencies

```
lib.rs
  ├── core.rs      (no deps)
  ├── tag.rs       (depends on: core)
  ├── spec.rs      (depends on: core, tag)
  ├── property.rs  (depends on: rand)
  ├── param.rs     (depends on: core)
  ├── report.rs    (depends on: core)
  ├── runner.rs    (depends on: core, report, spec)
  ├── coverage.rs  (depends on: core, coverage_raw)
  ├── coverage_raw.rs (depends on: serde, core)
  └── main.rs      (depends on: core, coverage, report, runner, clap)
```

No circular dependencies. Each module depends only on `core` and
possibly sibling modules.

---

## CLI Architecture

The `cargo-rvtest` binary (`src/main.rs`) is a thin wrapper:

1. Parse args with `clap`
2. If `--coverage`, delegate to `CoverageCollector`
3. Otherwise:
   - Run `cargo test` via `Command`
   - Parse stdout into `TestRun` via `parse_cargo_test_output()`
   - Render with the selected `TestReporter`
   - Exit with code 0 or 1

The spinner is an optional cosmetic thread that runs only when
stdout is a terminal.

---

## Coverage Architecture

```
CoverageCollector::collect()
    │
    ├── has_cargo_llvm_cov()?  ──►  run_via_cargo_llvm_cov()
    ├── has_llvm_tools()?      ──►  run_via_llvm_tools()
    ├── self_contained_profraw()?  ──►  run_via_raw_parser()
    └── (fallback)             ──►  run_via_sampler()
```

The self-contained parser (`coverage_raw.rs`) implements the raw
profile format directly:

```
RawProfile {
    Magic:     0xff6c70726f667281
    Version:   10
    Header:    16 × u64 = 128 bytes
    BinaryIds: variable
    Data:      [ProfileData × NumData] (64 bytes each)
    Counters:  [u64 × NumCounters]
    Names:     [u8 × NamesSize]
}
```

LLVM 22+ raw profile format.  Coverage is computed as the ratio
of non-zero counters to total counters per function and overall.
