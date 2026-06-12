use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rvtest::core::{RunnerConfig, TestRun};
use rvtest::param::{parametrize, parametrize_named};
use rvtest::property::{any, check};
use rvtest::report::{CompactReporter, JsonReporter, PrettyReporter, TapReporter, TestReporter};
use rvtest::runner::TestRunner;
use rvtest::spec::describe;

#[test]
fn rvtest_spec() {
    describe("Spec")
        .describe("execution")
            .it("passes when all tests pass", || {
                describe("Math")
                    .it("adds", || assert_eq!(2 + 2, 4))
                    .it("subtracts", || assert_eq!(5 - 3, 2))
                    .run()
                    .assert_all_pass();
            })
            .it("reports failures", || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    describe("Failing")
                        .it("fails", || panic!("intentional failure"))
                        .run()
                        .assert_all_pass();
                }));
                assert!(result.is_err(), "assert_all_pass should panic on failure");
            })
            .it("supports tags and timeout", || {
                describe("Tagged")
                    .it("passing", || {})
                    .tag("smoke")
                    .timeout(Duration::from_secs(1))
                    .run()
                    .assert_all_pass();
            })
            .it("retries flaky tests", || {
                let counter = AtomicU32::new(0);
                describe("Flaky")
                    .it("succeeds on retry", move || {
                        let prev = counter.fetch_add(1, Ordering::SeqCst);
                        if prev == 0 {
                            panic!("first attempt fails");
                        }
                    })
                    .retries(2)
                    .run()
                    .assert_all_pass();
            })
            .it("runs before_all hook", || {
                let ran = Arc::new(AtomicBool::new(false));
                let setup = Arc::clone(&ran);
                describe("Setup")
                    .before_all(move || {
                        setup.store(true, Ordering::SeqCst);
                    })
                    .it("hook executed", move || {
                        assert!(ran.load(Ordering::SeqCst), "before_all should have run");
                    })
                    .run()
                    .assert_all_pass();
            })
        .tag("spec")
        .run()
        .assert_all_pass();
}

#[test]
fn rvtest_parametrized() {
    describe("Parametrized")
        .it("runs all cases", || {
            let results = parametrize("add", [(1, 1, 2), (0, 0, 0), (-1, 1, 0)], |(a, b, exp)| {
                assert_eq!(a + b, *exp);
            });
            assert!(results.iter().all(|c| c.status.is_passed()));
            assert_eq!(results.len(), 3);
        })
        .it("supports named cases", || {
            let results = parametrize_named(
                "parse",
                [("empty", ""), ("valid", "42")],
                |input| {
                    if !input.is_empty() {
                        assert!(input.parse::<i32>().is_ok());
                    }
                },
            );
            assert!(results.iter().all(|c| c.status.is_passed()));
        })
        .tag("param")
        .run()
        .assert_all_pass();
}

#[test]
fn rvtest_property() {
    describe("Property")
        .it("passes for valid properties", || {
            check(
                "identity with zero",
                any::<i32>(),
                |a: &i32| a + 0 == *a,
            );
        })
        .it("detects falsified properties", || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                check(
                    "intentionally false",
                    any::<i32>(),
                    |_: &i32| false,
                );
            }));
            assert!(result.is_err(), "check should panic on falsified property");
        })
        .tag("property")
        .run()
        .assert_all_pass();
}

#[test]
fn rvtest_runner() {
    describe("Runner")
        .it("executes specs with custom config", || {
            let config = RunnerConfig {
                parallel: false,
                verbose: true,
                ..RunnerConfig::default()
            };

            let run = TestRunner::new(config)
                .add_spec(describe("Runner test").it("works", || {}))
                .run();

            assert!(run.success());
            assert_eq!(run.total(), 1);
        })
        .tag("runner")
        .run()
        .assert_all_pass();
}

#[test]
fn rvtest_architecture() {
    use rvtest::arch::arch_check;

    // Verify that the core module does not depend on reporting or coverage.
    // This is a real architectural constraint we want to enforce.
    arch_check()
        .module("core").may_not_depend_on(&["report", "coverage", "runner"])
        .module("report").may_depend_on(&["core"])
        .module("report").may_not_depend_on(&["coverage", "runner"])
        .module("runner").may_depend_on(&["core", "report"])
        .all_modules().must_not_have_cycles()
        .assert_all_pass();
}

#[test]
fn rvtest_reporters() {
    describe("Reporters")
        .it("pretty reporter shows summary", || {
            let report = PrettyReporter::new(false).colour(false).report(&TestRun::new());
            assert!(report.contains("0 passed"), "should show pass count");
            assert!(report.contains("0 failed"), "should show fail count");
        })
        .it("tap reporter outputs correct header", || {
            let report = TapReporter.report(&TestRun::new());
            assert!(report.starts_with("1..0"));
        })
        .it("compact reporter shows counts", || {
            let report = CompactReporter.report(&TestRun::new());
            assert!(report.contains("Results:"), "should have results line: {report:?}");
            assert!(report.contains("0/0"), "should show zero counts");
        })
        .it("json reporter is valid", || {
            let report = JsonReporter.report(&TestRun::new());
            assert!(report.contains(r#""success":true"#));
            assert!(report.contains(r#""suites":["#));
        })
        .tag("report")
        .run()
        .assert_all_pass();
}
