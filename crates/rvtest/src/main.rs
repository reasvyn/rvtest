use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use clap::Parser;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};

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

    // === Watch mode ===
    /// Re-run tests automatically when source files change.
    #[arg(long = "watch")]
    watch: bool,

    // === Flaky detection ===
    /// Detect flaky tests by running the suite multiple times.
    /// Optionally specify the number of runs: --detect-flaky=20 (default 10).
    #[arg(long = "detect-flaky", default_missing_value = "10", num_args = 0..=1, require_equals = false, default_value = "0")]
    detect_flaky: u32,

    // === Snapshot options ===
    /// Update all snapshots to match current output.
    #[arg(long = "update-all")]
    update_all: bool,

    /// Review pending snapshots interactively.
    #[arg(long = "review")]
    review: bool,

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
    // Skip the subcommand name when invoked via `cargo rvtest`.
    // Cargo always passes the subcommand name as argv[1].
    let args: Vec<String> = std::env::args().collect();
    let raw_args: Vec<String> = if args.len() > 1 && args[1] == "rvtest" {
        let mut a = vec![args[0].clone()];
        a.extend_from_slice(&args[2..]);
        a
    } else {
        args
    };

    let args = Cli::parse_from(raw_args);

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

    // === Snapshot config ===
    if args.update_all {
        rvtest::snapshot::set_update_all(true);
    }

    // === Watch mode ===
    if args.watch {
        let filter = args.filter.clone();
        let format = args.format.clone();
        let verbose = args.verbose;
        watch_loop(filter, format, verbose);
        return;
    }

    // === Flaky detection ===
    if args.detect_flaky > 0 {
        let filter = args.filter.clone();
        let n = args.detect_flaky;
        let verbose = args.verbose;
        detect_flaky(filter, n, verbose);
        return;
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

// ---------------------------------------------------------------------------
// Watch mode
// ---------------------------------------------------------------------------

fn watch_loop(filter: Option<String>, format_str: String, verbose: bool) {
    let done = Arc::new(AtomicBool::new(false));
    let format: ReportFormat = format_str.parse().unwrap_or(ReportFormat::Pretty);

    // Build watcher for src/ and tests/.
    let (tx, rx) = std::sync::mpsc::channel::<Result<Event, notify::Error>>();
    let mut watcher = match RecommendedWatcher::new(tx, Config::default()) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("Error: cannot start file watcher: {e}");
            std::process::exit(1);
        }
    };
    for dir in &["src", "tests"] {
        if Path::new(dir).exists() {
            let _ = watcher.watch(Path::new(dir), RecursiveMode::Recursive);
        }
    }

    // Initial run.
    run_and_print(&filter, &format, verbose);
    eprint!("  Watching src/, tests/ for changes... press 'q' to quit.\n\n");

    // Register Ctrl-C handler via libc.
    #[cfg(unix)]
    {
        unsafe {
            libc::signal(libc::SIGINT, sigint_handler as *const () as libc::sighandler_t);
        }
    }

    let debounce = Duration::from_millis(300);
    let mut pending = false;

    loop {
        if done.load(Ordering::SeqCst) {
            break;
        }

        // Collect events within debounce window.
        let deadline = Instant::now() + debounce;
        while Instant::now() < deadline && !done.load(Ordering::SeqCst) {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(Ok(_)) => pending = true,
                Ok(Err(_)) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        // Check for 'q' keypress (non-blocking on Unix).
        #[cfg(unix)]
        if !done.load(Ordering::SeqCst) && check_quit_key() {
            eprintln!("Quitting.");
            break;
        }

        if !pending && !done.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }

        pending = false;

        if done.load(Ordering::SeqCst) {
            break;
        }

        eprintln!("  Change detected — re-running tests...\n");
        run_and_print(&filter, &format, verbose);
        eprint!("\n  Watching... press 'q' to quit.\n\n");
    }
}

#[cfg(unix)]
unsafe extern "C" fn sigint_handler(_: libc::c_int) {
    // Default Ctrl-C handling — process will terminate.
}

/// Run `cargo test` N times and report which tests are flaky.
fn detect_flaky(filter: Option<String>, num_runs: u32, verbose: bool) {
    use std::collections::HashMap;

    eprintln!("\n  🔍 Running test suite {num_runs} times to detect flaky tests...\n");

    let mut results: HashMap<String, (u32, u32)> = HashMap::new(); // name → (passes, total)

    for run in 1..=num_runs {
        if verbose {
            eprint!("  Run {run}/{num_runs}... ");
        }

        let test_run = run_cargo_test(filter.as_deref());

        for suite in &test_run.suites {
            for test in &suite.tests {
                // Only track tests that actually ran (ignore skipped/ignored).
                if test.status.is_skipped() {
                    continue;
                }
                let entry = results.entry(test.name.clone()).or_insert((0, 0));
                entry.1 += 1; // total
                if test.status.is_passed() {
                    entry.0 += 1; // passes
                }
            }
        }

        if verbose {
            let passed = test_run.total_passed();
            let failed = test_run.total_failed();
            eprintln!("{passed} passed, {failed} failed");
        }
    }

    // Report flaky tests.
    eprintln!();
    let mut flaky_found = false;

    let mut sorted: Vec<_> = results.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    for (name, (passes, total)) in &sorted {
        let rate = *passes as f64 / *total as f64 * 100.0;
        if rate < 100.0 {
            flaky_found = true;
            eprintln!(
                "  ⚠  {name:<60} {passes}/{total} passes ({rate:.0}%)"
            );
        }
    }

    if !flaky_found {
        eprintln!("  ✅ No flaky tests detected — every test passed on all {num_runs} runs.");
    }
    eprintln!();
}

fn run_and_print(filter: &Option<String>, format: &ReportFormat, verbose: bool) {
    let run = run_cargo_test(filter.as_deref());
    let report = render(format, verbose, &run);
    println!("{report}");
}

#[cfg(unix)]
fn check_quit_key() -> bool {
    use std::os::fd::AsRawFd;
    let fd = io::stdin().as_raw_fd();
    let mut fds: libc::fd_set = unsafe { std::mem::zeroed() };
    unsafe { libc::FD_SET(fd, &mut fds) };
    let mut tv = libc::timeval { tv_sec: 0, tv_usec: 0 };
    let ret = unsafe { libc::select(fd + 1, &mut fds, std::ptr::null_mut(), std::ptr::null_mut(), &mut tv) };
    if ret > 0 {
        let mut buf = [0u8; 1];
        if io::stdin().read_exact(&mut buf).is_ok() && (buf[0] == b'q' || buf[0] == b'Q') {
            return true;
        }
    }
    false
}

fn render(format: &ReportFormat, verbose: bool, run: &TestRun) -> String {
    let reporter: Box<dyn TestReporter> = match format {
        ReportFormat::Pretty => Box::new(report::PrettyReporter::new(verbose)),
        ReportFormat::Tap => Box::new(report::TapReporter),
        ReportFormat::Junit => Box::new(report::JunitReporter::new()),
        ReportFormat::Json => Box::new(report::JsonReporter),
        ReportFormat::Compact => Box::new(report::CompactReporter),
        ReportFormat::Github => Box::new(report::GithubReporter),
    };
    reporter.report(run)
}
