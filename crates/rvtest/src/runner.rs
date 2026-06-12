use std::time::{Instant, SystemTime};

use crate::core::{ReportFormat, RunnerConfig, TestRun, TestSuite};
use crate::report::{self, TestReporter};
use crate::spec::Spec;

/// A configurable test runner that collects specs and executes them.
///
/// `TestRunner` provides a unified entry point for running tests defined with
/// [`Spec`], applying filtering, parallel execution, retries, and timeouts
/// according to a [`RunnerConfig`].
///
/// # Example
///
/// ```ignore
/// use rvtest::runner::TestRunner;
/// use rvtest::spec::describe;
/// use rvtest::core::RunnerConfig;
///
/// let runner = TestRunner::new(RunnerConfig::default())
///     .add_spec(describe("Math")
///         .it("adds", || assert_eq!(2 + 2, 4))
///     );
///
/// let run = runner.run();
/// assert!(run.success());
/// ```
pub struct TestRunner {
    config: RunnerConfig,
    specs: Vec<Spec>,
}

impl TestRunner {
    /// Create a new runner with the given configuration.
    pub fn new(config: RunnerConfig) -> Self {
        TestRunner { config, specs: Vec::new() }
    }

    /// Add a test spec to the runner.
    pub fn add_spec(mut self, spec: Spec) -> Self {
        self.specs.push(spec);
        self
    }

    /// Add multiple test specs at once.
    pub fn add_specs(mut self, specs: impl IntoIterator<Item = Spec>) -> Self {
        self.specs.extend(specs);
        self
    }

    /// Execute all registered specs and return a [`TestRun`] with aggregated
    /// results.
    pub fn run(mut self) -> TestRun {
        let start_time = SystemTime::now();
        let wall_start = Instant::now();

        let mut suites = Vec::new();

        let specs = std::mem::take(&mut self.specs);
        for spec in specs {
            let suite = self.run_spec(spec);
            suites.push(suite);
        }

        let duration = wall_start.elapsed();

        TestRun {
            suites,
            start_time,
            end_time: SystemTime::now(),
            duration,
        }
    }

    fn run_spec(&self, spec: Spec) -> TestSuite {
        spec.run_with_config(&self.config)
    }

    /// Render the test run results using the configured report format.
    pub fn report(&self, run: &TestRun) -> String {
        let reporter: Box<dyn TestReporter> = match self.config.format {
            ReportFormat::Pretty => Box::new(report::PrettyReporter::new(self.config.verbose)),
            ReportFormat::Tap => Box::new(report::TapReporter),
            ReportFormat::Junit => Box::new(report::JunitReporter::new()),
            ReportFormat::Json => Box::new(report::JsonReporter),
            ReportFormat::Compact => Box::new(report::CompactReporter),
        };
        reporter.report(run)
    }
}

/// Convenience function to run specs with default configuration.
///
/// Equivalent to `TestRunner::new(RunnerConfig::default()).add_specs(specs).run()`.
pub fn run_tests(specs: impl IntoIterator<Item = Spec>) -> TestRun {
    TestRunner::new(RunnerConfig::default())
        .add_specs(specs)
        .run()
}

/// Run specs and print the report to stdout.
///
/// Exits the process with code `0` on success or `1` on failure.
pub fn run_and_exit(specs: impl IntoIterator<Item = Spec>) -> ! {
    let config = RunnerConfig::default();
    let run = run_tests(specs);
    let report = render_report_with_config(&config, &run);
    println!("{report}");
    std::process::exit(if run.success() { 0 } else { 1 });
}

fn render_report_with_config(config: &RunnerConfig, run: &TestRun) -> String {
    let reporter: Box<dyn TestReporter> = match config.format {
        ReportFormat::Pretty => Box::new(report::PrettyReporter::new(config.verbose)),
        ReportFormat::Tap => Box::new(report::TapReporter),
        ReportFormat::Junit => Box::new(report::JunitReporter::new()),
        ReportFormat::Json => Box::new(report::JsonReporter),
        ReportFormat::Compact => Box::new(report::CompactReporter),
    };
    reporter.report(run)
}


