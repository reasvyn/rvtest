//! **rvtest** — A Next Level Testing Framework for Rust.
//!
//! `rvtest` extends Rust's built-in testing capabilities with:
//!
//! - **BDD-style specs** — organise tests with `describe` / `it` blocks,
//!   nested hierarchies, tags, timeouts, and retries.
//! - **Property-based testing** — verify invariants over randomly generated
//!   inputs, with automatic counterexample shrinking.
//! - **Parametrized tests** — run the same test logic against multiple
//!   inputs without boilerplate.
//! - **Rich reporting** — Pretty (human-readable with colour), TAP, JUnit
//!   XML, JSON, and Compact formats.
//! - **Code coverage** — measure line/function/region coverage via LLVM
//!   instrumentation (`cargo rvtest --coverage`).
//! - **Configurable runner** — parallel execution, name and tag filtering,
//!   fail-fast, configurable timeouts and retries.
//!
//! # Usage inside `#[test]` (recommended)
//!
//! Rvtest is designed to be used inside standard `#[test]` functions.
//! Build a spec with [`describe`] and [`it`](spec::Spec::it), call
//! [`run`](spec::Spec::run), then verify with [`assert_all_pass`](core::TestSuite::assert_all_pass):
//!
//! ```ignore
//! use rvtest::spec::describe;
//!
//! #[test]
//! fn calculator_tests() {
//!     describe("Calculator")
//!         .it("adds two positive numbers", || {
//!             assert_eq!(2 + 2, 4);
//!         })
//!         .it("subtracts", || {
//!             assert_eq!(5 - 3, 2);
//!         })
//!         .tag("arithmetic")
//!         .run()
//!         .assert_all_pass();
//! }
//! ```
//!
//! If any spec fails, `assert_all_pass` will panic with a detailed report,
//! which causes the `#[test]` to fail naturally — no need for `main()`.
//!
//! # Property-based testing inside `#[test]`
//!
//! ```ignore
//! use rvtest::property::{check, any};
//!
//! #[test]
//! fn addition_is_commutative() {
//!     check("commutativity", any::<i32>(), |a: &i32| {
//!         let b: i32 = 42;  // fixed second operand
//!         a + b == b + *a
//!     });
//! }
//! ```
//!
//! # Parametrized tests inside `#[test]`
//!
//! ```ignore
//! use rvtest::param::parametrize;
//!
//! #[test]
//! fn addition_cases() {
//!     for case in parametrize("add", [(1, 1, 2), (0, 0, 0), (-1, 1, 0)], |(a, b, exp)| {
//!         assert_eq!(a + b, *exp);
//!     }) {
//!         assert!(case.status.is_passed(), "{} failed", case.name);
//!     }
//! }
//! ```
//!
//! # CLI usage (`cargo rvtest`)
//!
//! The `cargo-rvtest` binary runs specs defined in your project and
//! produces formatted output. See `cargo rvtest --help`.

pub mod core;
pub mod coverage;
pub mod coverage_raw;
pub mod param;
pub mod property;
pub mod report;
pub mod runner;
pub mod spec;
pub mod tag;

/// Re-export of the optional proc-macro crate.
///
/// Enabled via the `macros` feature:
///
/// ```toml
/// [dependencies]
/// rvtest = { version = "0.1", features = ["macros"] }
/// ```
///
/// Then use:
///
/// ```ignore
/// use rvtest::describe;
/// ```
#[cfg(feature = "macros")]
pub use rvtest_macros::{after_all, before_all, describe, it, retries, tag, timeout};

/// The `prelude` module re-exports the most commonly used types and
/// functions for convenience.
pub mod prelude {
    pub use crate::core::{CoverageFormat, CoverageReport, ReportFormat, RunnerConfig, TestRun, TestStatus, TestSuite};
    pub use crate::coverage::{CoverageCollector, CoverageConfig};
    pub use crate::param::{parametrize, parametrize_named};
    pub use crate::property::{any, check, check_with, PropertyConfig, Strategy};
    pub use crate::runner::TestRunner;
    pub use crate::spec::{describe, Spec};
}
