# Contributing to rvtest

First off, thank you for considering contributing!  Every issue,
pull request, and discussion makes this project better.

---

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [How to Contribute](#how-to-contribute)
- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Pull Request Process](#pull-request-process)
- [Coding Standards](#coding-standards)
- [Testing](#testing)
- [Documentation](#documentation)

---

## Code of Conduct

This project is governed by the [Contributor Covenant](CODE_OF_CONDUCT.md).
By participating, you are expected to uphold this code.

---

## How to Contribute

### Report a Bug

Open an issue at https://github.com/reasvyn/rvtest/issues/new.
Include:

- `rvtest` version (check `Cargo.lock`)
- Rust version (`rustc --version`)
- A minimal reproduction
- Expected vs actual behaviour

### Suggest a Feature

Check the [roadmap](docs/roadmap.md) first — your idea might already
be planned.  If not, open an issue with:

- A clear description of the problem
- How you envision the solution
- Any alternative approaches you considered

### Submit a Pull Request

1. Fork the repo
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Make your changes
4. Run the tests (`cargo test`)
5. Ensure zero warnings (`cargo check`)
6. Submit a PR against `main`

---

## Development Setup

```bash
# Clone your fork
git clone https://github.com/YOUR_USERNAME/rvtest.git
cd rvtest

# Run the tests
cargo test

# Run the CLI
cargo run --bin cargo-rvtest -- -v

# Run coverage
cargo run --bin cargo-rvtest -- --coverage
```

### Prerequisites

- **Rust 1.96+** (edition 2024)
- **No external LLVM tools required** — the self-contained coverage
  parser works out of the box

---

## Project Structure

```
crates/rvtest/src/
  lib.rs           -- Public API, prelude, module declarations
  core.rs          -- Core types (TestSuite, TestCase, TestStatus, ...)
  spec.rs          -- BDD spec builder (describe / it)
  runner.rs        -- TestRunner, execution, run_tests helpers
  report.rs        -- TestReporter trait and all format implementations
  param.rs         -- Parametrized tests
  property.rs      -- Property-based testing
  tag.rs           -- Tag and name filtering
  coverage.rs      -- Coverage collector with multi-strategy fallback
  coverage_raw.rs  -- Pure-Rust .profraw parser
  main.rs          -- CLI entry point (cargo-rvtest binary)

docs/
  roadmap.md       -- Planned features and timeline
  architecture.md  -- Internal architecture
  philosophy.md    -- Design principles
  conventions.md   -- Code conventions
  getting-started.md -- Tutorial

tests/
  integration.rs   -- Dogfooded integration tests
```

---

## Pull Request Process

1. **One feature per PR.**  If you have multiple unrelated changes,
   submit separate PRs.

2. **Keep PRs small.**  A focused PR is easier to review and merge.
   Aim for < 400 lines changed.

3. **Write tests.**  New features should include integration tests
   using rvtest's own BDD API (dogfooding).  Bug fixes should include
   a regression test.

4. **Update docs.**  If you change the public API or add a feature,
   update the relevant doc comments and any documentation files.

5. **Pass CI.**  Ensure `cargo check` and `cargo test` pass with zero
   warnings.

6. **Sign-off.**  Your commits should include a `Signed-off-by` line
   (`git commit -s`) to certify that you wrote the code or have the
   right to contribute it.

---

## Coding Standards

See [docs/conventions.md](docs/conventions.md) for the full guide.

Key points:

- **Edition 2024**, fmt with defaults
- **Zero warnings** — `cargo check` must be clean
- **No `unsafe`** unless absolutely necessary and documented
- **Functions return `Result<T, String>`** for fallible operations
- **Doc comments** on all public items
- **Use existing patterns** — look at similar code before writing new

---

## Testing

```bash
# Full test suite
cargo test

# Specific test
cargo test rvtest_spec

# CLI integration
cargo run --bin cargo-rvtest -- -v

# Coverage
cargo run --bin cargo-rvtest -- --coverage

# All formats
cargo run --bin cargo-rvtest -- -F json
cargo run --bin cargo-rvtest -- -F compact
cargo run --bin cargo-rvtest -- -F tap
```

All integration tests use `rvtest`'s own BDD API (dogfooding).

---

## Documentation

- Public API items must have doc comments (`///`)
- Module-level docs (`//!`) describe the module's purpose and
  provide usage examples
- Code examples in doc comments use `ignore` or `no_run` since
  they require the rvtest crate to be imported
- Documentation files in `docs/` should follow the same style
  as existing files

---

## Questions?

Open a [discussion](https://github.com/reasvyn/rvtest/discussions)
or ask in the issue tracker.
