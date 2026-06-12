use std::fmt::Write;

use crate::core::{TestRun, TestStatus};

// ---------------------------------------------------------------------------
// Reporter trait
// ---------------------------------------------------------------------------

/// Renders a [`TestRun`] into a human- or machine-readable string.
pub trait TestReporter {
    /// Format the entire test run into a string.
    fn report(&self, run: &TestRun) -> String;
}

// ---------------------------------------------------------------------------
// PrettyReporter — colourful, human-friendly output
// ---------------------------------------------------------------------------

fn sep_line() -> String {
    "─".repeat(58)
}

/// Human-readable reporter with optional colour and verbosity.
pub struct PrettyReporter {
    verbose: bool,
    use_colour: bool,
}

impl PrettyReporter {
    /// Create a new `PrettyReporter`.
    ///
    /// When `verbose` is `true`, passing tests are also listed (by default
    /// only failures are shown in detail).
    pub fn new(verbose: bool) -> Self {
        PrettyReporter { verbose, use_colour: true }
    }

    /// Enable or disable ANSI colour codes.
    pub fn colour(mut self, enabled: bool) -> Self {
        self.use_colour = enabled;
        self
    }
}

impl TestReporter for PrettyReporter {
    fn report(&self, run: &TestRun) -> String {
        let mut out = String::new();

        for suite in &run.suites {
            // Suite header with centered text
            let header = format!("  {}  ", suite.name);
            let pad = 58usize.saturating_sub(header.chars().count());
            let left = pad / 2;
            let right = pad - left;
            let mut line = String::new();
            for _ in 0..left { line.push('─'); }
            line.push_str(&header);
            for _ in 0..right { line.push('─'); }
            let _ = writeln!(out, "\n{}", line);

            for test in &suite.tests {
                if !self.verbose && test.status.is_passed() {
                    continue;
                }

                let (icon, label) = status_badge(&test.status, self.use_colour);
                let dur = format_duration(test.duration);

                let _ = writeln!(out, "  {icon} {label}  {}  {}", test.name, dur);

                if let TestStatus::Failed { ref reason, ref location } = test.status {
                    if let Some(loc) = location {
                        let loc_str = format_location(loc, self.use_colour);
                        let _ = writeln!(out, "         {}  {}", dim("at", self.use_colour), loc_str);
                    }
                    for line in reason.lines() {
                        let _ = writeln!(out, "         {}", line);
                    }
                }

                if let TestStatus::TimedOut { duration, ref location } = test.status {
                    let _ = writeln!(out, "         {} {duration:?}", dim("timed out after", self.use_colour));
                    if let Some(loc) = location {
                        let _ = writeln!(out, "         {}  {}", dim("at", self.use_colour), format_location(loc, self.use_colour));
                    }
                }
            }
        }

        // Summary
        let total = run.total();
        let passed = run.total_passed();
        let failed = run.total_failed();
        let skipped = run.total_skipped();
        let dur_s = run.duration.as_secs_f64();

        let _ = writeln!(out);
        let _ = writeln!(out, "{}", sep_line());

        let status_text = if run.success() {
            coloured("ok", "32", self.use_colour)
        } else {
            coloured("FAILED", "31", self.use_colour)
        };

        let passed_s = coloured_count(passed, "passed", "32", self.use_colour);
        let failed_s = coloured_count(failed, "failed", "31", self.use_colour);
        let skipped_s = coloured_count(skipped, "skipped", "33", self.use_colour);

        let _ = writeln!(
            out,
            "  {status_text}  {passed_s} · {failed_s} · {skipped_s}  │  {total} total  │  {dur_s:.2}s",
        );

        out
    }
}

fn status_badge(status: &TestStatus, colour: bool) -> (String, String) {
    match status {
        TestStatus::Passed => (coloured("✓", "32", colour), coloured("PASS", "32", colour)),
        TestStatus::Failed { .. } => (coloured("✗", "31", colour), coloured("FAIL", "31", colour)),
        TestStatus::Skipped { .. } => (coloured("–", "33", colour), coloured("SKIP", "33", colour)),
        TestStatus::TimedOut { .. } => (coloured("⊗", "31", colour), coloured("TIMEOUT", "31", colour)),
    }
}

fn format_duration(d: std::time::Duration) -> String {
    let secs = d.as_secs_f64();
    if secs >= 1.0 {
        format!("{secs:.2}s")
    } else {
        format!("{:.1}ms", secs * 1000.0)
    }
}

fn format_location(loc: &crate::core::SourceLocation, colour: bool) -> String {
    let s = match loc.column {
        Some(col) => format!("{}:{}:{}", loc.file, loc.line, col),
        None => format!("{}:{}", loc.file, loc.line),
    };
    coloured(&s, "36", colour)
}

fn coloured(s: &str, code: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b[{code}m{s}\x1b[0m")
    } else {
        s.to_owned()
    }
}

fn dim(s: &str, enabled: bool) -> String {
    coloured(s, "2", enabled)
}

fn coloured_count(n: usize, label: &str, colour_code: &str, enabled: bool) -> String {
    format!("{} {}", coloured(&n.to_string(), colour_code, enabled), label)
}

// ---------------------------------------------------------------------------
// TapReporter — Test Anything Protocol
// ---------------------------------------------------------------------------

/// Reporter that emits TAP (Test Anything Protocol) output.
///
/// TAP is a simple line-based protocol widely used in the Perl and
/// JavaScript ecosystems and supported by many CI tools.
pub struct TapReporter;

impl TestReporter for TapReporter {
    fn report(&self, run: &TestRun) -> String {
        let mut out = String::new();

        let total = run.total();
        let _ = writeln!(out, "1..{total}");

        let mut index = 0;
        for suite in &run.suites {
            for test in &suite.tests {
                index += 1;
                let ok = if test.status.is_passed() { "ok" } else { "not ok" };
                let duration_ms = test.duration.as_secs_f64() * 1000.0;

                let _ = writeln!(out, "{ok} {index} - {} [{duration_ms:.1}ms]", test.name);

                if let TestStatus::Failed { ref reason, .. } = test.status {
                    for line in reason.lines() {
                        let _ = writeln!(out, "  {line}");
                    }
                }

                if let TestStatus::TimedOut { duration, .. } = test.status {
                    let _ = writeln!(out, "  # TIMEOUT after {duration:?}");
                }

                if let TestStatus::Skipped { ref reason } = test.status {
                    let reason = reason.as_deref().unwrap_or("no reason given");
                    let _ = writeln!(out, "  # SKIP {reason}");
                }
            }
        }

        out
    }
}

// ---------------------------------------------------------------------------
// JunitReporter — JUnit XML for CI integration
// ---------------------------------------------------------------------------

/// Reporter that emits JUnit-compatible XML.
///
/// This format is understood by Jenkins, GitLab CI, GitHub Actions, and
/// most other CI systems.
pub struct JunitReporter {
    suite_name: String,
}

impl JunitReporter {
    /// Create a new `JunitReporter`.
    pub fn new() -> Self {
        JunitReporter { suite_name: "rvtest".to_owned() }
    }

    /// Override the top-level suite name in the XML output.
    pub fn suite_name(mut self, name: &str) -> Self {
        self.suite_name = name.to_owned();
        self
    }
}

impl Default for JunitReporter {
    fn default() -> Self {
        Self::new()
    }
}

impl TestReporter for JunitReporter {
    fn report(&self, run: &TestRun) -> String {
        let mut out = String::new();
        let _ = writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        let _ = writeln!(
            out,
            r#"<testsuites name="{}" tests="{}" failures="{}" skipped="{}" time="{:.3}">"#,
            self.suite_name,
            run.total(),
            run.total_failed(),
            run.total_skipped(),
            run.duration.as_secs_f64(),
        );

        for suite in &run.suites {
            let _ = writeln!(
                out,
                r#"  <testsuite name="{}" tests="{}" failures="{}" skipped="{}" time="{:.3}">"#,
                escape_xml(&suite.name),
                suite.len(),
                suite.failed().count(),
                suite.skipped().count(),
                suite.duration.as_secs_f64(),
            );

            for test in &suite.tests {
                let classname = test.suite.as_deref().unwrap_or("root");
                let dur_s = test.duration.as_secs_f64();

                match &test.status {
                    TestStatus::Passed => {
                        let _ = writeln!(
                            out,
                            r#"    <testcase classname="{}" name="{}" time="{:.3}" />"#,
                            escape_xml(classname),
                            escape_xml(&test.name),
                            dur_s,
                        );
                    }
                    TestStatus::Failed { reason, .. } => {
                        let _ = writeln!(
                            out,
                            r#"    <testcase classname="{}" name="{}" time="{:.3}">"#,
                            escape_xml(classname),
                            escape_xml(&test.name),
                            dur_s,
                        );
                        let _ = writeln!(
                            out,
                            r#"      <failure message="{}" type="AssertionError"><![CDATA[{}]]></failure>"#,
                            escape_xml(reason),
                            reason,
                        );
                        let _ = writeln!(out, "    </testcase>");
                    }
                    TestStatus::Skipped { reason } => {
                        let msg = reason.as_deref().unwrap_or("skipped");
                        let _ = writeln!(
                            out,
                            r#"    <testcase classname="{}" name="{}" time="{:.3}">"#,
                            escape_xml(classname),
                            escape_xml(&test.name),
                            dur_s,
                        );
                        let _ = writeln!(
                            out,
                            r#"      <skipped message="{}" />"#,
                            escape_xml(msg),
                        );
                        let _ = writeln!(out, "    </testcase>");
                    }
                    TestStatus::TimedOut { duration: to, .. } => {
                        let _ = writeln!(
                            out,
                            r#"    <testcase classname="{}" name="{}" time="{:.3}">"#,
                            escape_xml(classname),
                            escape_xml(&test.name),
                            dur_s,
                        );
                        let _ = writeln!(
                            out,
                            r#"      <failure message="timed out after {:?}" type="TimeoutError" />"#,
                            to,
                        );
                        let _ = writeln!(out, "    </testcase>");
                    }
                }
            }

            let _ = writeln!(out, "  </testsuite>");
        }

        let _ = writeln!(out, "</testsuites>");
        out
    }
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ---------------------------------------------------------------------------
// JsonReporter — machine-readable JSON
// ---------------------------------------------------------------------------

/// Reporter that emits results as a JSON object.
pub struct JsonReporter;

impl TestReporter for JsonReporter {
    fn report(&self, run: &TestRun) -> String {
        let mut out = String::new();

        let success = if run.success() { "true" } else { "false" };
        out.push_str(&format!(
            r#"{{"success":{},"total":{},"passed":{},"failed":{},"skipped":{},"duration_secs":{:.3},"suites":["#,
            success,
            run.total(),
            run.total_passed(),
            run.total_failed(),
            run.total_skipped(),
            run.duration.as_secs_f64(),
        ));

        for (si, suite) in run.suites.iter().enumerate() {
            if si > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                r#"{{"name":"{}","duration_secs":{:.3},"tests":["#,
                escape_json(&suite.name),
                suite.duration.as_secs_f64(),
            ));

            for (ti, test) in suite.tests.iter().enumerate() {
                if ti > 0 {
                    out.push(',');
                }
                let (status_str, reason) = match &test.status {
                    TestStatus::Passed => ("passed", None),
                    TestStatus::Failed { reason, .. } => ("failed", Some(reason.as_str())),
                    TestStatus::Skipped { reason } => ("skipped", reason.as_deref()),
                    TestStatus::TimedOut { .. } => ("timed_out", None),
                };

                out.push_str(&format!(
                    r#"{{"name":"{}","status":"{}","duration_secs":{:.3}"#,
                    escape_json(&test.name),
                    status_str,
                    test.duration.as_secs_f64(),
                ));

                if let Some(r) = reason {
                    out.push_str(&format!(r#","reason":"{}""#, escape_json(r)));
                }

                if !test.parameters.is_empty() {
                    out.push_str(r#","parameters":{"#);
                    for (pi, (k, v)) in test.parameters.iter().enumerate() {
                        if pi > 0 {
                            out.push(',');
                        }
                        out.push_str(&format!(r#""{}":"{}""#, escape_json(k), escape_json(v)));
                    }
                    out.push('}');
                }

                out.push_str("]}");
            }

            out.push('}');
        }

        out.push(']');
        out.push('}');
        out
    }
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

// ---------------------------------------------------------------------------
// CompactReporter — one line per test
// ---------------------------------------------------------------------------

/// Minimal, single-line-per-test reporter suitable for quick feedback.
pub struct CompactReporter;

impl TestReporter for CompactReporter {
    fn report(&self, run: &TestRun) -> String {
        let mut out = String::new();
        let total = run.total();
        let passed = run.total_passed();
        let failed = run.total_failed();
        let skipped = run.total_skipped();

        for suite in &run.suites {
            for test in &suite.tests {
                let status = match test.status {
                    TestStatus::Passed => "PASS",
                    TestStatus::Failed { .. } => "FAIL",
                    TestStatus::Skipped { .. } => "SKIP",
                    TestStatus::TimedOut { .. } => "TIMEOUT",
                };
                let dur = format_duration(test.duration);
                let _ = writeln!(out, "{status}  {dur:>7}  {}", test.name);
            }
        }

        let _ = writeln!(
            out,
            "\nResults: {passed}/{total} passed, {failed} failed, {skipped} skipped  ({:.2}s)",
            run.duration.as_secs_f64(),
        );

        out
    }
}

// ---------------------------------------------------------------------------
// GithubReporter — GitHub Actions annotations
// ---------------------------------------------------------------------------

/// Reporter that emits GitHub Actions-compatible `::error` / `::warning`
/// annotations for test failures.
///
/// Each failed or timed-out test produces one `::error` line with the
/// source file, line number, and failure message.  Passing tests are
/// silently ignored.
///
/// # Example output
///
/// ```text
/// ::error file=tests/demo.rs,line=42,title=Calculator :: adds — assertion failed
/// ```
pub struct GithubReporter;

impl TestReporter for GithubReporter {
    fn report(&self, run: &TestRun) -> String {
        let mut out = String::new();
        let mut passed = 0usize;
        let mut failed = 0usize;
        let mut skipped = 0usize;

        for suite in &run.suites {
            for test in &suite.tests {
                match &test.status {
                    TestStatus::Passed => passed += 1,
                    TestStatus::Skipped { .. } => skipped += 1,
                    TestStatus::Failed { reason, location } => {
                        failed += 1;
                        let file = location
                            .as_ref()
                            .map(|l| escape_github(l.file.as_str()))
                            .unwrap_or_else(|| "unknown".to_string());
                        let line = location
                            .as_ref()
                            .map(|l| l.line.to_string())
                            .unwrap_or_else(|| "1".to_string());
                        let title = escape_github(&test.name);
                        let msg = escape_github(reason);
                        let _ = writeln!(
                            out,
                            "::error file={file},line={line},title={title}::{msg}"
                        );
                    }
                    TestStatus::TimedOut { duration, location } => {
                        failed += 1;
                        let file = location
                            .as_ref()
                            .map(|l| escape_github(l.file.as_str()))
                            .unwrap_or_else(|| "unknown".to_string());
                        let line = location
                            .as_ref()
                            .map(|l| l.line.to_string())
                            .unwrap_or_else(|| "1".to_string());
                        let title = escape_github(&test.name);
                        let msg = format!("timed out after {duration:?}");
                        let _ = writeln!(
                            out,
                            "::error file={file},line={line},title={title}::{msg}"
                        );
                    }
                }
            }
        }

        let total = run.total();
        let _ = writeln!(
            out,
            "rvtest: {passed}/{total} passed, {failed} failed, {skipped} skipped  ({:.2}s)",
            run.duration.as_secs_f64(),
        );

        out
    }
}

fn escape_github(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\n', "%0A")
        .replace('\r', "%0D")
}
