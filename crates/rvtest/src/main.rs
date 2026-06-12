use std::io::{self, IsTerminal, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use clap::Parser;

use rvtest::core::{CoverageFormat, ReportFormat, TestCase, TestRun, TestStatus, TestSuite};
use rvtest::coverage::{CoverageCollector, CoverageConfig};
use rvtest::report::{self, TestReporter};

#[derive(Parser)]
#[command(
    name = "cargo-rvtest",
    about = "A Next Level Testing Framework for Rust",
    version,
    long_about = "rvtest is A Next Level Testing Framework for Rust.\n\n\
                   rvtest extends Rust's built-in testing with BDD specs, \
                   property-based testing, parametrized tests, and rich reporting. \
                   Use `cargo rvtest` to run tests or `cargo rvtest --coverage` \
                   for code coverage analysis."
)]
struct Cli {
    // === Test options ===
    /// Filter test names by substring (case-insensitive).
    #[arg(short = 'f', long = "filter")]
    filter: Option<String>,

    /// Only run tests carrying all of these tags (can be specified multiple times).
    #[arg(short = 't', long = "tag")]
    include_tags: Vec<String>,

    /// Skip tests carrying any of these tags (can be specified multiple times).
    #[arg(short = 'E', long = "exclude-tag")]
    exclude_tags: Vec<String>,

    /// Number of retries for flaky tests.
    #[arg(short = 'r', long = "retries", default_value = "0")]
    retries: u32,

    /// Default per-test timeout in seconds.
    #[arg(long = "timeout")]
    timeout_secs: Option<f64>,

    /// Run tests sequentially.
    #[arg(long = "no-parallel")]
    no_parallel: bool,

    /// Maximum number of threads for parallel execution.
    #[arg(long = "max-threads", default_value = "0")]
    max_threads: usize,

    /// Output format: pretty, tap, junit, json, compact.
    #[arg(short = 'F', long = "format", default_value = "pretty")]
    format: String,

    /// Stop after the first failure.
    #[arg(long = "fail-fast")]
    fail_fast: bool,

    /// Seed for randomised features.
    #[arg(long = "seed")]
    seed: Option<u64>,

    /// Show verbose output (list all tests).
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,

    // === Coverage options ===
    /// Enable code coverage collection via LLVM instrumentation.
    #[arg(long = "coverage")]
    coverage: bool,

    /// Coverage output format: summary, html, lcov, json, cobertura.
    #[arg(long = "coverage-format", default_value = "summary")]
    coverage_format: String,

    /// Directory to place coverage artifacts.
    #[arg(long = "coverage-dir", default_value = "target/coverage")]
    coverage_dir: PathBuf,

    /// Minimum line-coverage percentage (fails if below threshold).
    #[arg(long = "coverage-min")]
    coverage_min: Option<f64>,

    /// Open coverage report in browser (implies --coverage).
    #[arg(long = "coverage-open")]
    coverage_open: bool,
}

fn main() {
    let args = Cli::parse();

    // === Coverage mode ===
    if args.coverage || args.coverage_open {
        let cov_format: CoverageFormat = args.coverage_format.parse().unwrap_or_else(|e| {
            eprintln!("{e}, falling back to 'summary'");
            CoverageFormat::Summary
        });

        let cov_config = CoverageConfig {
            enabled: true,
            format: cov_format,
            output_dir: args.coverage_dir.clone(),
            min_threshold: args.coverage_min,
            open_report: args.coverage_open,
            ..Default::default()
        };

        let collector = CoverageCollector::new(cov_config);
        match collector.collect() {
            Ok(report) => {
                println!(
                    "Coverage: {:.1}% lines, {:.1}% functions, {:.1}% regions",
                    report.line_coverage,
                    report.function_coverage,
                    report.region_coverage,
                );
                if let Some(path) = &report.report_path {
                    println!("Report: {}", path.display());
                }
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Coverage collection failed:\n{e}");
                std::process::exit(1);
            }
        }
    }

    // === Test mode ===
    let format: ReportFormat = args.format.parse().unwrap_or_else(|e| {
        eprintln!("{e}, falling back to 'pretty'");
        ReportFormat::Pretty
    });

    let run = run_cargo_test(args.filter.as_deref());

    let report = render(&format, args.verbose, &run);
    println!("{report}");
    std::process::exit(if run.success() { 0 } else { 1 });
}

/// Run `cargo test` and parse the output into a structured [`TestRun`].
fn run_cargo_test(filter: Option<&str>) -> TestRun {
    let start = SystemTime::now();
    let wall_start = Instant::now();

    let mut cmd = Command::new("cargo");
    cmd.arg("test").arg("--color=never");

    if let Some(f) = filter {
        cmd.arg("--").arg(f);
    }

    let is_tty = io::stdout().is_terminal();
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let spinner_handle = std::thread::spawn(move || {
        if !is_tty {
            r.store(false, Ordering::SeqCst);
            return;
        }
        let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        let mut i = 0;
        while r.load(Ordering::SeqCst) {
            print!("\r  {} {}  {} running...", frames[i], dim("cargo test"), dim("tests"));
            io::stdout().flush().ok();
            i = (i + 1) % frames.len();
            std::thread::sleep(Duration::from_millis(80));
        }
    });

    let output = match cmd.output() {
        Ok(o) => {
            running.store(false, Ordering::SeqCst);
            let _ = spinner_handle.join();
            if is_tty {
                print!("\r");
                io::stdout().flush().ok();
            }
            o
        }
        Err(e) => {
            running.store(false, Ordering::SeqCst);
            let _ = spinner_handle.join();
            if is_tty {
                print!("\r");
                io::stdout().flush().ok();
            }
            eprintln!("Error: failed to run `cargo test`: {e}");
            std::process::exit(1);
        }
    };

    let duration = wall_start.elapsed();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let suites = parse_cargo_test_output(&stdout);

    TestRun {
        suites,
        start_time: start,
        end_time: SystemTime::now(),
        duration,
    }
}

fn dim(s: &str) -> String {
    format!("\x1b[2m{s}\x1b[0m")
}

/// Parse the stdout of `cargo test` into a [`TestSuite`].
///
/// Recognises lines in the form:
/// ```text
/// test <name> ... ok
/// test <name> ... FAILED
/// test <name> ... ignored
/// ```
/// plus the summary line for totals.
fn parse_cargo_test_output(stdout: &str) -> Vec<TestSuite> {
    let mut tests = Vec::new();
    let mut failure_details: Vec<String> = Vec::new();
    let mut in_failures = false;

    for line in stdout.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("---- ") && trimmed.ends_with(" stdout ----") {
            in_failures = true;
            continue;
        }

        if trimmed == "failures:" {
            in_failures = false;
            continue;
        }

        if in_failures && !trimmed.is_empty() && !trimmed.starts_with("----") {
            failure_details.push(trimmed.to_owned());
        }

        if let Some(rest) = trimmed.strip_prefix("test ") {
            if let Some((name, rest)) = rest.split_once(" ... ") {
                let status = if rest.starts_with("ok") {
                    TestStatus::Passed
                } else if rest.starts_with("FAILED") {
                    TestStatus::Failed { reason: String::new(), location: None }
                } else if rest.starts_with("ignored") {
                    let reason = rest.strip_prefix("ignored,")
                        .or_else(|| rest.strip_prefix("ignored"))
                        .map(|s| s.trim().trim_start_matches("...").trim().to_owned())
                        .filter(|s| !s.is_empty());
                    TestStatus::Skipped { reason }
                } else {
                    continue;
                };

                tests.push(TestCase {
                    name: name.to_owned(),
                    suite: Some("cargo test".to_owned()),
                    tags: Vec::new(),
                    status,
                    duration: Duration::ZERO,
                    assertions: 0,
                    location: None,
                    parameters: Vec::new(),
                });
            }
        }
    }

    // Attach failure details to failed tests by matching names
    if !failure_details.is_empty() {
        let mut detail_iter = failure_details.into_iter();
        for test in &mut tests {
            if matches!(test.status, TestStatus::Failed { .. }) {
                let detail: String = detail_iter.by_ref()
                    .take_while(|l| !l.starts_with("test ") && !l.starts_with("----") && !l.starts_with("\n") && !l.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                if !detail.is_empty() {
                    test.status = TestStatus::Failed {
                        reason: detail,
                        location: None,
                    };
                }
            }
        }
    }

    if tests.is_empty() {
        return Vec::new();
    }

    vec![TestSuite {
        name: "cargo test".to_owned(),
        description: None,
        tests,
        duration: Duration::ZERO,
    }]
}

fn render(format: &ReportFormat, verbose: bool, run: &TestRun) -> String {
    let reporter: Box<dyn TestReporter> = match format {
        ReportFormat::Pretty => Box::new(report::PrettyReporter::new(verbose)),
        ReportFormat::Tap => Box::new(report::TapReporter),
        ReportFormat::Junit => Box::new(report::JunitReporter::new()),
        ReportFormat::Json => Box::new(report::JsonReporter),
        ReportFormat::Compact => Box::new(report::CompactReporter),
    };
    reporter.report(run)
}
