use std::fmt;
use std::time::{Duration, SystemTime};

/// A location in source code where a test is defined or an assertion failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocation {
    /// The file path.
    pub file: String,
    /// The line number (1-indexed).
    pub line: u32,
    /// The optional column number.
    pub column: Option<u32>,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.column {
            Some(col) => write!(f, "{}:{}:{}", self.file, self.line, col),
            None => write!(f, "{}:{}", self.file, self.line),
        }
    }
}

/// The outcome of a single test case execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestStatus {
    /// The test completed successfully without panicking.
    Passed,
    /// The test panicked or returned an error.
    Failed {
        /// A human-readable description of the failure.
        reason: String,
        /// Where the failure originated, if known.
        location: Option<SourceLocation>,
    },
    /// The test was skipped, optionally with a reason.
    Skipped {
        /// Why the test was skipped.
        reason: Option<String>,
    },
    /// The test exceeded its allotted time budget.
    TimedOut {
        /// The maximum duration allowed.
        duration: Duration,
        /// Where the test is defined, if known.
        location: Option<SourceLocation>,
    },
}

impl TestStatus {
    /// Returns `true` if the status represents a passing outcome.
    pub fn is_passed(&self) -> bool {
        matches!(self, TestStatus::Passed)
    }

    /// Returns `true` if the status represents any kind of failure (including timeout).
    pub fn is_failed(&self) -> bool {
        matches!(self, TestStatus::Failed { .. } | TestStatus::TimedOut { .. })
    }

    /// Returns `true` if the test was skipped.
    pub fn is_skipped(&self) -> bool {
        matches!(self, TestStatus::Skipped { .. })
    }
}

impl fmt::Display for TestStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestStatus::Passed => write!(f, "PASSED"),
            TestStatus::Failed { reason, .. } => write!(f, "FAILED: {reason}"),
            TestStatus::Skipped { reason: Some(r) } => write!(f, "SKIPPED: {r}"),
            TestStatus::Skipped { reason: None } => write!(f, "SKIPPED"),
            TestStatus::TimedOut { duration, .. } => {
                write!(f, "TIMED OUT after {duration:?}")
            }
        }
    }
}

/// A single test case with its metadata and execution result.
#[derive(Debug, Clone)]
pub struct TestCase {
    /// The human-readable name of the test.
    pub name: String,
    /// The name of the parent suite, if any.
    pub suite: Option<String>,
    /// Tags attached to this test for filtering and organisation.
    pub tags: Vec<String>,
    /// The outcome of executing the test.
    pub status: TestStatus,
    /// How long the test took to execute.
    pub duration: Duration,
    /// How many assertions were performed (best-effort count).
    pub assertions: u64,
    /// Where the test was defined in source code.
    pub location: Option<SourceLocation>,
    /// Named parameters supplied to a parametrized test.
    pub parameters: Vec<(String, String)>,
}

impl TestCase {
    /// Create a new test case with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        TestCase {
            name: name.into(),
            suite: None,
            tags: Vec::new(),
            status: TestStatus::Passed,
            duration: Duration::ZERO,
            assertions: 0,
            location: None,
            parameters: Vec::new(),
        }
    }
}

/// A collection of related test cases that share a common context.
#[derive(Debug, Clone)]
pub struct TestSuite {
    /// The name of this suite (e.g. a module or `describe` block name).
    pub name: String,
    /// An optional description of what this suite covers.
    pub description: Option<String>,
    /// The test cases belonging to this suite.
    pub tests: Vec<TestCase>,
    /// Total wall-clock duration for all tests in this suite.
    pub duration: Duration,
}

impl TestSuite {
    /// Create a new empty suite with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        TestSuite {
            name: name.into(),
            description: None,
            tests: Vec::new(),
            duration: Duration::ZERO,
        }
    }

    /// Returns the number of tests in this suite.
    pub fn len(&self) -> usize {
        self.tests.len()
    }

    /// Returns `true` if this suite contains no tests.
    pub fn is_empty(&self) -> bool {
        self.tests.is_empty()
    }

    /// Returns an iterator over tests that passed.
    pub fn passed(&self) -> impl Iterator<Item = &TestCase> {
        self.tests.iter().filter(|t| t.status.is_passed())
    }

    /// Returns an iterator over tests that failed.
    pub fn failed(&self) -> impl Iterator<Item = &TestCase> {
        self.tests.iter().filter(|t| t.status.is_failed())
    }

    /// Returns an iterator over tests that were skipped.
    pub fn skipped(&self) -> impl Iterator<Item = &TestCase> {
        self.tests.iter().filter(|t| t.status.is_skipped())
    }

    /// Returns `true` if every test in this suite passed.
    pub fn success(&self) -> bool {
        self.failed().count() == 0
    }

    /// Panics with a detailed failure report if any test in this suite
    /// did not pass. Designed for use inside `#[test]` functions.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[test]
    /// fn my_tests() {
    ///     describe("Calculator")
    ///         .it("adds", || assert_eq!(2 + 2, 4))
    ///         .run()
    ///         .assert_all_pass();
    /// }
    /// ```
    pub fn assert_all_pass(&self) {
        let failed: Vec<&TestCase> = self.failed().collect();
        if !failed.is_empty() {
            let mut msg = format!(
                "{} test(s) failed in suite '{}':\n",
                failed.len(),
                self.name,
            );
            for t in &failed {
                let dur_ms = t.duration.as_secs_f64() * 1000.0;
                let reason = match &t.status {
                    TestStatus::Failed { reason, .. } => reason.as_str(),
                    TestStatus::TimedOut { .. } => "timed out",
                    _ => "unknown",
                };
                msg.push_str(&format!("  ✗ {} [{dur_ms:.1}ms] — {reason}\n", t.name));
            }
            panic!("{msg}");
        }
    }
}

/// Aggregated results from an entire test run consisting of one or more suites.
#[derive(Debug, Clone)]
pub struct TestRun {
    /// The suites that were executed.
    pub suites: Vec<TestSuite>,
    /// Wall-clock time the run started.
    pub start_time: SystemTime,
    /// Wall-clock time the run finished.
    pub end_time: SystemTime,
    /// Total wall-clock duration of the run.
    pub duration: Duration,
}

impl TestRun {
    /// Create a new `TestRun` starting now.
    pub fn new() -> Self {
        TestRun {
            suites: Vec::new(),
            start_time: SystemTime::now(),
            end_time: SystemTime::now(),
            duration: Duration::ZERO,
        }
    }

    /// Returns the total number of test cases across all suites.
    pub fn total(&self) -> usize {
        self.suites.iter().map(|s| s.tests.len()).sum()
    }

    /// Returns the number of passed test cases.
    pub fn total_passed(&self) -> usize {
        self.suites.iter().flat_map(|s| s.tests.iter()).filter(|t| t.status.is_passed()).count()
    }

    /// Returns the number of failed test cases (including timeouts).
    pub fn total_failed(&self) -> usize {
        self.suites.iter().flat_map(|s| s.tests.iter()).filter(|t| t.status.is_failed()).count()
    }

    /// Returns the number of skipped test cases.
    pub fn total_skipped(&self) -> usize {
        self.suites.iter().flat_map(|s| s.tests.iter()).filter(|t| t.status.is_skipped()).count()
    }

    /// Returns `true` if every test passed.
    pub fn success(&self) -> bool {
        self.total_failed() == 0
    }

    /// Returns an iterator over all test cases that failed.
    pub fn all_failed(&self) -> impl Iterator<Item = &TestCase> {
        self.suites.iter().flat_map(|s| s.failed())
    }
}

impl Default for TestRun {
    fn default() -> Self {
        Self::new()
    }
}

/// The output format used when rendering test results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReportFormat {
    /// Human-readable, colourised output (default).
    #[default]
    Pretty,
    /// Test Anything Protocol — machine-parseable line-based format.
    Tap,
    /// JUnit XML — widely supported by CI systems.
    Junit,
    /// JSON output — suitable for programmatic consumption.
    Json,
    /// Compact single-line-per-test output.
    Compact,
    /// GitHub Actions annotations.
    Github,
}

impl std::str::FromStr for ReportFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pretty" | "human" => Ok(Self::Pretty),
            "tap" => Ok(Self::Tap),
            "junit" | "xml" => Ok(Self::Junit),
            "json" => Ok(Self::Json),
            "compact" => Ok(Self::Compact),
            "github" | "gh" => Ok(Self::Github),
            _ => Err(format!("unknown report format: {s}")),
        }
    }
}

/// Global configuration for a test run.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    /// Only run tests whose name contains this string.
    pub filter: Option<String>,
    /// Only run tests carrying *all* of these tags.
    pub include_tags: Vec<String>,
    /// Skip tests carrying *any* of these tags.
    pub exclude_tags: Vec<String>,
    /// Default number of retries for flaky tests.
    pub default_retries: u32,
    /// Default per-test timeout.
    pub default_timeout: Option<Duration>,
    /// Whether to run tests in parallel.
    pub parallel: bool,
    /// Maximum number of threads for parallel execution.
    pub max_threads: usize,
    /// Output format for results.
    pub format: ReportFormat,
    /// Stop after the first failure.
    pub fail_fast: bool,
    /// Seed for randomised features (property testing, shuffle).
    pub seed: Option<u64>,
    /// Show detailed output for each test.
    pub verbose: bool,
}

impl Default for RunnerConfig {
    fn default() -> Self {
        RunnerConfig {
            filter: None,
            include_tags: Vec::new(),
            exclude_tags: Vec::new(),
            default_retries: 0,
            default_timeout: None,
            parallel: true,
            max_threads: num_cpus(),
            format: ReportFormat::Pretty,
            fail_fast: false,
            seed: None,
            verbose: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Coverage types
// ---------------------------------------------------------------------------

/// Output format for coverage reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverageFormat {
    /// Plain-text summary printed to stdout.
    #[default]
    Summary,
    /// HTML report with line-level detail.
    Html,
    /// LCOV tracefile (for IDE integration, Coveralls, etc.).
    Lcov,
    /// Machine-readable JSON.
    Json,
    /// Cobertura XML (for Jenkins, GitLab, etc.).
    Cobertura,
}

impl std::str::FromStr for CoverageFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "summary" | "text" => Ok(Self::Summary),
            "html" => Ok(Self::Html),
            "lcov" | "tracefile" => Ok(Self::Lcov),
            "json" => Ok(Self::Json),
            "cobertura" | "xml" => Ok(Self::Cobertura),
            _ => Err(format!("unknown coverage format: {s}")),
        }
    }
}

/// Aggregated coverage metrics for a codebase.
#[derive(Debug, Clone)]
pub struct CoverageReport {
    /// Percentage of lines covered (0.0 – 100.0).
    pub line_coverage: f64,
    /// Percentage of functions covered.
    pub function_coverage: f64,
    /// Percentage of regions (basic blocks) covered.
    pub region_coverage: f64,
    /// The format the full report was generated in.
    pub format: CoverageFormat,
    /// Path to the generated report file, if applicable.
    pub report_path: Option<std::path::PathBuf>,
}

/// Heuristic for the number of available CPUs.
fn num_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4)
}
