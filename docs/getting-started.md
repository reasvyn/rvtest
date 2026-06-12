# Getting Started with rvtest

> **A Next Level Testing Framework for Rust**

---

## Installation

Add `rvtest` as a dev-dependency:

```bash
cargo add --dev rvtest
```

Or add it manually to `Cargo.toml`:

```toml
[dev-dependencies]
rvtest = "0.1.0"
```

To use the CLI:

```bash
cargo install rvtest
```

Then run:

```bash
cargo rvtest
```

---

## Writing Your First Spec

Create a test file (`tests/calculator.rs` or inline in `src/`):

```rust
use rvtest::spec::describe;

#[test]
fn calculator_tests() {
    describe("Calculator")
        .it("adds two numbers", || {
            assert_eq!(2 + 2, 4);
        })
        .it("subtracts", || {
            assert_eq!(5 - 3, 2);
        })
        .run()
        .assert_all_pass();
}
```

Run it:

```bash
cargo test
```

Or with the `rvtest` CLI for formatted output:

```bash
cargo rvtest
```

---

## Feature Tutorials

### 1. BDD Specs with Nesting

```rust
use rvtest::spec::describe;

#[test]
fn math_spec() {
    describe("Math")
        .describe("addition")
            .it("positive + positive", || assert_eq!(2 + 2, 4))
            .it("negative + negative", || assert_eq!(-2 + -3, -5))
        .describe("multiplication")
            .it("zero property", || assert_eq!(5 * 0, 0))
            .tag("core")              // tag for filtering
            .timeout(std::time::Duration::from_secs(1))
        .run()
        .assert_all_pass();
}
```

**Tips:**
- Use `.tag("name")` to attach metadata for filtering
- Use `.timeout(duration)` to set per-suite time limits
- Nested blocks inherit parent tags and timeouts

### 2. Lifecycle Hooks

```rust
use std::sync::{Arc, Mutex};
use rvtest::spec::describe;

#[test]
fn with_setup() {
    let db = Arc::new(Mutex::new(Vec::new()));
    let setup = Arc::clone(&db);

    describe("Database")
        .before_all(move || {
            setup.lock().unwrap().push("connected".to_string());
        })
        .it("is connected", move || {
            assert_eq!(db.lock().unwrap()[0], "connected");
        })
        .after_all(|| {
            // cleanup
        })
        .run()
        .assert_all_pass();
}
```

### 3. Retrying Flaky Tests

```rust
use std::sync::atomic::{AtomicU32, Ordering};
use rvtest::spec::describe;

#[test]
fn flaky_retry() {
    let counter = AtomicU32::new(0);

    describe("Network")
        .it("eventually succeeds", move || {
            let prev = counter.fetch_add(1, Ordering::SeqCst);
            if prev < 2 {
                panic!("transient failure {}", prev);
            }
        })
        .retries(3)   // up to 3 retries
        .run()
        .assert_all_pass();
}
```

### 4. Property-Based Testing

```rust
use rvtest::property::{check, any};

#[test]
fn identity_property() {
    check("addition with zero", any::<i32>(), |a: &i32| {
        *a + 0 == *a
    });
}
```

Custom strategies:

```rust
use rvtest::property::{Strategy, any, check};
use rand::RngCore;

struct EvenStrategy;

impl Strategy<i32> for EvenStrategy {
    fn generate(&self, rng: &mut dyn RngCore) -> i32 {
        rng.next_u32() as i32 & !1  // force even
    }
}

#[test]
fn even_numbers() {
    check("even numbers are divisible by 2", EvenStrategy, |n: &i32| {
        n % 2 == 0
    });
}
```

### 5. Parametrized Tests

```rust
use rvtest::param::parametrize;

#[test]
fn addition_cases() {
    let cases = parametrize(
        "add",
        [(1, 1, 2), (0, 0, 0), (-1, 1, 0), (-2, -2, -4)],
        |(a, b, expected)| {
            assert_eq!(a + b, *expected);
        },
    );
    assert!(cases.iter().all(|c| c.status.is_passed()));
}
```

Named parametrization:

```rust
use rvtest::param::parametrize_named;

#[test]
fn parse_cases() {
    let results = parametrize_named(
        "parse",
        [("empty", ""), ("valid_number", "42")],
        |input| {
            if !input.is_empty() {
                assert!(input.parse::<i32>().is_ok());
            }
        },
    );
    assert!(results.iter().all(|c| c.status.is_passed()));
}
```

---

## CLI Usage

```bash
# Basic run
cargo rvtest

# Filter by name
cargo rvtest -f calculator

# Verbose (show passing tests)
cargo rvtest -v

# Output as JSON
cargo rvtest -F json

# Coverage
cargo rvtest --coverage

# Coverage with HTML report
cargo rvtest --coverage --coverage-format json

# Tag filtering
cargo rvtest --tag smoke
cargo rvtest --tag math --exclude-tag slow

# Retry flaky tests
cargo rvtest --retries 3

# Stop on first failure
cargo rvtest --fail-fast

# Sequential execution
cargo rvtest --no-parallel
```

---

## IDE Integration

Since `rvtest` works inside standard `#[test]` functions, your IDE
(CLion, VS Code, RustRover) can run individual specs via the normal
test runner controls.  No special plugin required.

---

## CI Integration

```yaml
# .github/workflows/ci.yml
steps:
  - uses: actions/checkout@v4
  - uses: actions-rust-lang/setup-rust-toolchain@v1

  - name: Run tests with rvtest
    run: cargo rvtest

  - name: Coverage
    run: cargo rvtest --coverage
```

For JUnit XML in CI:

```bash
cargo rvtest -F junit > results.xml
```

Then use GitHub Actions' `dorny/test-reporter` or GitLab's JUnit
integration to parse the results.

---

## Next Steps

- Read the [architecture](architecture.md) doc for a deep dive
- Check the [roadmap](roadmap.md) for upcoming features
- Browse the source: each module in `src/` has extensive doc comments
