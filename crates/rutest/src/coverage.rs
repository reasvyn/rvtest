use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use crate::core::{CoverageFormat, CoverageReport};

/// Configuration for a coverage collection run.
#[derive(Debug, Clone)]
pub struct CoverageConfig {
    /// Whether coverage is enabled at all.
    pub enabled: bool,
    /// Desired output format.
    pub format: CoverageFormat,
    /// Directory to place coverage artifacts.
    pub output_dir: PathBuf,
    /// Minimum acceptable line-coverage percentage. `collect()` returns an
    /// error if coverage falls below this threshold.
    pub min_threshold: Option<f64>,
    /// Open the report in the system browser after generation.
    pub open_report: bool,
    /// Any extra CLI arguments to forward to the test runner.
    pub extra_test_args: Vec<String>,
    /// Sampling interval in milliseconds for the built-in sampler.
    pub sample_interval_ms: u64,
}

impl Default for CoverageConfig {
    fn default() -> Self {
        CoverageConfig {
            enabled: false,
            format: CoverageFormat::Summary,
            output_dir: PathBuf::from("target/coverage"),
            min_threshold: None,
            open_report: false,
            extra_test_args: Vec::new(),
            sample_interval_ms: 5,
        }
    }
}

/// Collects code coverage data.
///
/// Tries three strategies in order:
///
/// 1. **cargo-llvm-cov** — best results, requires `cargo install cargo-llvm-cov`.
/// 2. **Manual llvm-tools** — uses `-Cinstrument-coverage`, `llvm-profdata`,
///    `llvm-cov`. Requires `rustup component add llvm-tools-preview`.
/// 3. **Built-in sampler** — lightweight statistical sampling via `ptrace`
///    + `addr2line`. Works without any LLVM tools.
pub struct CoverageCollector {
    config: CoverageConfig,
}

impl CoverageCollector {
    pub fn new(config: CoverageConfig) -> Self {
        CoverageCollector { config }
    }

    pub fn collect(&self) -> Result<CoverageReport, String> {
        if self.has_cargo_llvm_cov() {
            return self.run_via_cargo_llvm_cov();
        }
        if self.has_llvm_tools() {
            return self.run_via_llvm_tools();
        }
        // Lightweight profraw parser (pure-Rust, no external tools).
        if self.self_contained_profraw() {
            return self.run_via_raw_parser();
        }
        // Fallback: built-in sampler.
        self.run_via_sampler()
    }

    // ------------------------------------------------------------------
    // Tool detection
    // ------------------------------------------------------------------

    fn has_cargo_llvm_cov(&self) -> bool {
        Command::new("cargo")
            .args(["llvm-cov", "--help"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn has_llvm_tools(&self) -> bool {
        self.find_tool("llvm-profdata").is_some()
            && self.find_tool("llvm-cov").is_some()
    }

    fn has_addr2line(&self) -> bool {
        self.find_tool("addr2line").is_some()
    }

    fn find_tool(&self, name: &str) -> Option<PathBuf> {
        if let Ok(path) = which(name) {
            return Some(path);
        }
        // Search rustup toolchain dirs.
        if let Ok(home) = std::env::var("RUSTUP_HOME") {
            if let Ok(output) = Command::new("rustup").args(["default"]).output() {
                if output.status.success() {
                    let tc = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    for sub in ["lib/rustlib/x86_64-unknown-linux-gnu/bin", "bin"] {
                        let p = PathBuf::from(&home).join("toolchains").join(&tc).join(sub).join(name);
                        if p.exists() {
                            return Some(p);
                        }
                    }
                }
            }
        }
        None
    }

    // ------------------------------------------------------------------
    // Strategy 1: cargo-llvm-cov
    // ------------------------------------------------------------------

    fn run_via_cargo_llvm_cov(&self) -> Result<CoverageReport, String> {
        let out_dir = &self.config.output_dir;
        let format_flag = match self.config.format {
            CoverageFormat::Summary => "--summary-only",
            CoverageFormat::Html => "--html",
            CoverageFormat::Lcov => "--lcov",
            CoverageFormat::Json => "--json",
            CoverageFormat::Cobertura => "--cobertura",
        };

        let mut cmd = Command::new("cargo");
        cmd.args(["llvm-cov", "--all-targets", format_flag]);
        if !matches!(self.config.format, CoverageFormat::Summary) {
            cmd.arg("--output-dir").arg(out_dir);
        }
        if !self.config.extra_test_args.is_empty() {
            cmd.arg("--").args(&self.config.extra_test_args);
        }

        let status = cmd.status().map_err(|e| format!("cargo-llvm-cov: {e}"))?;
        if !status.success() {
            return Err("cargo-llvm-cov returned non-zero exit".into());
        }

        let report_path = if !matches!(self.config.format, CoverageFormat::Summary) {
            Some(out_dir.join(report_filename(self.config.format)))
        } else {
            None
        };
        let (line, func, region) = self.parse_llvm_cov_summary()?;
        self.check_threshold_and_open(CoverageReport {
            line_coverage: line,
            function_coverage: func,
            region_coverage: region,
            format: self.config.format,
            report_path,
        })
    }

    fn parse_llvm_cov_summary(&self) -> Result<(f64, f64, f64), String> {
        let output = Command::new("cargo")
            .args(["llvm-cov", "--summary-only", "--all-targets"])
            .output()
            .map_err(|e| format!("cargo-llvm-cov summary: {e}"))?;
        if !output.status.success() {
            return Ok((0.0, 0.0, 0.0));
        }
        Ok(parse_coverage_percentages(&String::from_utf8_lossy(&output.stdout)))
    }

    // ------------------------------------------------------------------
    // Strategy 2: manual llvm-tools
    // ------------------------------------------------------------------

    fn run_via_llvm_tools(&self) -> Result<CoverageReport, String> {
        let out_dir = &self.config.output_dir;
        let profraw_dir = out_dir.join("profraw");
        for d in [&profraw_dir, out_dir] {
            std::fs::create_dir_all(d).map_err(|e| format!("mkdir {d:?}: {e}"))?;
        }

        let llvm_profile = profraw_dir.join("default_%p_%m.profraw");
        let build = self.run_cargo_test_no_run(Some(&llvm_profile))?;
        let binaries = parse_test_binaries(&build.stdout);

        if binaries.is_empty() {
            return Err("no test binaries produced".into());
        }

        for bin in &binaries {
            let s = Command::new(bin)
                .env("LLVM_PROFILE_FILE", llvm_profile.to_str().unwrap())
                .args(&self.config.extra_test_args)
                .status()
                .map_err(|e| format!("run {bin:?}: {e}"))?;
            if !s.success() {
                eprintln!("warning: {bin:?} exited non-zero");
            }
        }

        let merged = out_dir.join("merged.profdata");
        let profraws = glob_dir(&profraw_dir, "*.profraw")?;
        if profraws.is_empty() {
            return Err("no .profraw files produced".into());
        }

        let pdata = self.find_tool("llvm-profdata").unwrap();
        let mut mc = Command::new(&pdata);
        mc.args(["merge", "-sparse"]);
        for f in &profraws {
            mc.arg(f);
        }
        mc.arg("-o").arg(&merged);
        if !mc.status().map_err(|e| format!("llvm-profdata: {e}"))?.success() {
            return Err("llvm-profdata merge failed".into());
        }

        let cov = self.find_tool("llvm-cov").unwrap();
        let report = self.llvm_cov_report(&cov, &merged, &binaries)?;
        self.check_threshold_and_open(report)
    }

    fn llvm_cov_report(&self, cov: &Path, profdata: &Path, bins: &[PathBuf]) -> Result<CoverageReport, String> {
        match self.config.format {
            CoverageFormat::Summary => {
                let (l, f, r) = self.llvm_summary(cov, profdata, bins)?;
                Ok(CoverageReport { line_coverage: l, function_coverage: f, region_coverage: r, format: CoverageFormat::Summary, report_path: None })
            }
            _ => {
                let (l, f, r) = self.llvm_summary(cov, profdata, bins)?;
                let filename = report_filename(self.config.format);
                let path = self.config.output_dir.join(&filename);

                let fmt = match self.config.format {
                    CoverageFormat::Html => "html",
                    CoverageFormat::Lcov => "lcov",
                    CoverageFormat::Json => "text",
                    _ => return Err("format requires cargo-llvm-cov".into()),
                };

                let mut cmd = Command::new(cov);
                cmd.args(["show", "--format", fmt])
                    .arg("--instr-profile").arg(profdata);
                for b in bins { cmd.arg("--object").arg(b); }

                let out = cmd.stdout(Stdio::piped()).stderr(Stdio::inherit())
                    .output().map_err(|e| format!("llvm-cov: {e}"))?;

                // For HTML/LCOV/JSON write file; for summary write to stdout.
                std::fs::write(&path, &out.stdout)
                    .map_err(|e| format!("write {path:?}: {e}"))?;

                Ok(CoverageReport { line_coverage: l, function_coverage: f, region_coverage: r, format: self.config.format, report_path: Some(path) })
            }
        }
    }

    fn llvm_summary(&self, cov: &Path, profdata: &Path, bins: &[PathBuf]) -> Result<(f64, f64, f64), String> {
        let mut cmd = Command::new(cov);
        cmd.args(["report", "--summary-only", "--use-color=false"])
            .arg("--instr-profile").arg(profdata);
        for b in bins { cmd.arg("--object").arg(b); }
        let out = cmd.stdout(Stdio::piped()).stderr(Stdio::inherit())
            .output().map_err(|e| format!("llvm-cov report: {e}"))?;
        Ok(parse_coverage_percentages(&String::from_utf8_lossy(&out.stdout)))
    }

    // ------------------------------------------------------------------
    // Strategy 3: built-in sampler (ptrace + addr2line)
    // ------------------------------------------------------------------

    #[cfg(target_os = "linux")]
    fn run_via_sampler(&self) -> Result<CoverageReport, String> {
        if !self.has_addr2line() {
            return Err(
                "built-in sampler requires `addr2line` (install binutils).\n\
                 Or install one of:\n  \
                 cargo install cargo-llvm-cov\n  \
                 rustup component add llvm-tools-preview"
                .into()
            );
        }

        let out_dir = &self.config.output_dir;
        std::fs::create_dir_all(out_dir).map_err(|e| format!("mkdir {out_dir:?}: {e}"))?;

        // Build test binary.
        let build = self.run_cargo_test_no_run(None)?;
        let binaries = parse_test_binaries(&build.stdout);
        let binary = binaries.first().ok_or("no test binary produced")?;
        if !binary.exists() {
            return Err(format!("test binary not found: {binary:?}"));
        }

        // Sample with ptrace.
        let samples = sample_ips(binary, self.config.sample_interval_ms, &self.config.extra_test_args)?;
        if samples.is_empty() {
            return Err("no instruction pointer samples collected".into());
        }

        // Resolve to source lines.
        let locations = resolve_with_addr2line(binary, &samples)?;

        // Count all non-blank source lines for the project.
        let total_source = count_source_lines("src")?;
        let unique_hit: HashSet<(String, u64)> = locations.into_iter().collect();
        let hit_count = unique_hit.len();

        let line_cov = if total_source > 0 {
            (hit_count as f64 / total_source as f64 * 100.0).min(100.0)
        } else {
            0.0
        };

        // Function coverage: count unique functions from addr2line output.
        // addr2line returns "function_name at file:line" — we count unique function names.
        let function_cov = line_cov; // approximate for sampler.

        let report = CoverageReport {
            line_coverage: line_cov,
            function_coverage: function_cov,
            region_coverage: line_cov,
            format: self.config.format,
            report_path: None,
        };

        println!(
            "\n📊  Built-in sampler coverage (statistical):\n   \
             Lines hit: {hit_count} / {total_source} ({line_cov:.1}%)\n   \
             Samples: {} (interval: {}ms)\n",
            samples.len(),
            self.config.sample_interval_ms,
        );

        self.check_threshold_and_open(report)
    }

    #[cfg(not(target_os = "linux"))]
    fn run_via_sampler(&self) -> Result<CoverageReport, String> {
        Err("built-in sampler is only available on Linux.\n\
             Install one of:\n  \
             cargo install cargo-llvm-cov\n  \
             rustup component add llvm-tools-preview"
            .into())
    }

    // ------------------------------------------------------------------
    // Strategy 3a: self-contained raw profraw parser (pure Rust)
    // ------------------------------------------------------------------

    /// Returns `true` if the rustc version supports `-Cinstrument-coverage`
    /// with a profraw version we can parse.
    fn self_contained_profraw(&self) -> bool {
        // We support LLVM 22+ (rustc 1.96+). Check by trying to compile
        // with -Cinstrument-coverage — if it fails, we fall through.
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "--no-run", "--message-format=json"])
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-Cinstrument-coverage")
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Also check that the rustc version is new enough.
        let version_ok = std::env::var("RUSTC")
            .ok()
            .and_then(|_| None)
            .or_else(|| {
                Command::new("rustc")
                    .arg("--version")
                    .output()
                    .ok()
                    .and_then(|o| {
                        let s = String::from_utf8_lossy(&o.stdout);
                        // Parse "rustc 1.96.0" — we need >= 1.96
                        let v = s.split_whitespace().nth(1)?;
                        let parts: Vec<&str> = v.split('.').collect();
                        let major: u32 = parts.first()?.parse().ok()?;
                        let minor: u32 = parts.get(1)?.parse().ok()?;
                        Some((major, minor))
                    })
            })
            .map(|(major, minor)| major >= 1 && minor >= 96)
            .unwrap_or(false);

        if !version_ok {
            return false;
        }

        cmd.status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn run_via_raw_parser(&self) -> Result<CoverageReport, String> {
        let runner = crate::coverage_raw::RawCoverageRunner {
            output_dir: self.config.output_dir.clone(),
            extra_test_args: self.config.extra_test_args.clone(),
        };
        runner.run(self.config.format)
    }

    // ------------------------------------------------------------------
    // Shared helpers
    // ------------------------------------------------------------------

    fn run_cargo_test_no_run(&self, llvm_profile: Option<&Path>) -> Result<Output, String> {
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "--no-run", "--message-format=json"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        if let Some(prof) = llvm_profile {
            cmd.env("CARGO_INCREMENTAL", "0");
            cmd.env("RUSTFLAGS", "-Cinstrument-coverage");
            cmd.env("LLVM_PROFILE_FILE", prof.to_str().unwrap());
        }

        if !self.config.extra_test_args.is_empty() {
            cmd.arg("--").args(&self.config.extra_test_args);
        }

        cmd.output().map_err(|e| format!("cargo test --no-run: {e}"))
    }

    fn check_threshold_and_open(&self, report: CoverageReport) -> Result<CoverageReport, String> {
        if let Some(threshold) = self.config.min_threshold
            && report.line_coverage < threshold
        {
            return Err(format!(
                "coverage {:.1}% is below minimum {threshold:.1}%",
                report.line_coverage,
            ));
        }

        if self.config.open_report {
            if let Some(ref path) = report.report_path {
                open_in_browser(path);
            }
        }

        Ok(report)
    }
}

// ======================================================================
// Linux ptrace-based IP sampler
// ======================================================================

#[cfg(target_os = "linux")]
fn sample_ips(binary: &Path, interval_ms: u64, _extra_args: &[String]) -> Result<Vec<u64>, String> {
    use std::ffi::CString;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let bin_cstr = CString::new(binary.to_str().ok_or("invalid binary path")?)
        .map_err(|_| "binary path contains null byte")?;

    // Safety: ptrace/fork/waitpid are unsafe syscalls.
    let child_pid = unsafe { libc::fork() };
    if child_pid == -1 {
        return Err("fork failed".into());
    }

    if child_pid == 0 {
        // ---- CHILD ----
        unsafe {
            libc::ptrace(libc::PTRACE_TRACEME, 0, std::ptr::null_mut::<libc::c_void>(), std::ptr::null_mut::<libc::c_void>());
            // Stop immediately so parent can set up tracing.
            libc::raise(libc::SIGSTOP);
        }
        // exec the test binary.
        let args: Vec<CString> = std::iter::once(bin_cstr.clone())
            .chain(_extra_args.iter().map(|a| CString::new(a.as_bytes()).unwrap()))
            .collect();
        let mut argv: Vec<*const libc::c_char> = args.iter().map(|a| a.as_ptr()).collect();
        argv.push(std::ptr::null());
        unsafe {
            libc::execvp(bin_cstr.as_ptr(), argv.as_ptr());
            // If execvp returns, it failed.
            libc::_exit(1);
        }
    }

    // ---- PARENT ----
    let mut samples: Vec<u64> = Vec::new();
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    // Spawn a timer thread that periodically sends SIGSTOP.
    let timer = std::thread::spawn(move || {
        while r.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(interval_ms));
            if !r.load(Ordering::SeqCst) {
                break;
            }
            unsafe {
                libc::kill(child_pid, libc::SIGSTOP);
            }
        }
    });

    let mut status: libc::c_int = 0;

    // Wait for initial SIGSTOP from child.
    unsafe {
        libc::waitpid(child_pid, &mut status as *mut libc::c_int, 0);
    }

    let max_samples = 200_000u64;
    let mut sample_count = 0u64;

    loop {
        if sample_count >= max_samples {
            break;
        }

        // Wait for child to be stopped.
        unsafe {
            libc::waitpid(child_pid, &mut status as *mut libc::c_int, 0);
        }

        if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
            break;
        }

        let stop_signal = if libc::WIFSTOPPED(status) {
            libc::WSTOPSIG(status)
        } else {
            0
        };

        if stop_signal == libc::SIGSTOP {
            // Timer signal — sample IP.
            let mut regs: libc::user_regs_struct = unsafe { std::mem::zeroed() };
            let mut iov = libc::iovec {
                iov_base: &mut regs as *mut _ as *mut libc::c_void,
                iov_len: std::mem::size_of::<libc::user_regs_struct>(),
            };

            // PTRACE_GETREGSET with NT_PRSTATUS
            let res = unsafe {
                libc::ptrace(
                    libc::PTRACE_GETREGSET as libc::c_uint,
                    child_pid,
                    libc::NT_PRSTATUS as *mut libc::c_void,
                    &mut iov as *mut libc::iovec as *mut libc::c_void,
                )
            };

            if res == 0 {
                // x86_64: regs.rip; aarch64: regs.pc
                #[cfg(target_arch = "x86_64")]
                let ip = regs.rip;
                #[cfg(target_arch = "aarch64")]
                let ip = regs.pc;
                #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
                let ip = 0u64;

                if ip > 0 {
                    samples.push(ip);
                    sample_count += 1;
                }
            }

            // Continue with SIGCONT to resume.
            unsafe {
                libc::ptrace(
                    libc::PTRACE_CONT as libc::c_uint,
                    child_pid,
                    std::ptr::null_mut::<libc::c_void>(),
                    libc::SIGCONT as *mut libc::c_void,
                );
            }
        } else if stop_signal > 0 {
            // Pass through other signals.
            unsafe {
                libc::ptrace(
                    libc::PTRACE_CONT as libc::c_uint,
                    child_pid,
                    std::ptr::null_mut::<libc::c_void>(),
                    stop_signal as *mut libc::c_void,
                );
            }
        } else {
            // No signal — unexpected stop, just continue.
            unsafe {
                libc::ptrace(
                    libc::PTRACE_CONT as libc::c_uint,
                    child_pid,
                    std::ptr::null_mut::<libc::c_void>(),
                    std::ptr::null_mut::<libc::c_void>(),
                );
            }
        }
    }

    running.store(false, Ordering::SeqCst);
    let _ = timer.join();

    // Ensure child is dead.
    unsafe {
        libc::kill(child_pid, libc::SIGKILL);
        libc::waitpid(child_pid, &mut status as *mut libc::c_int, 0);
    }

    Ok(samples)
}

#[cfg(target_os = "linux")]
fn resolve_with_addr2line(binary: &Path, ips: &[u64]) -> Result<Vec<(String, u64)>, String> {


    // Deduplicate IPs before calling addr2line.
    let unique: Vec<u64> = {
        let mut v: Vec<u64> = ips.to_vec();
        v.sort();
        v.dedup();
        v
    };

    if unique.is_empty() {
        return Ok(Vec::new());
    }

    // Build addr2line arguments.
    let mut cmd = Command::new("addr2line");
    cmd.arg("-e").arg(binary);
    cmd.arg("-f").arg("-a"); // -f: show function names, -a: show addresses
    for ip in &unique {
        cmd.arg(format!("0x{ip:x}"));
    }

    let output = cmd.stdout(Stdio::piped()).stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("addr2line: {e}"))?;

    if !output.status.success() {
        return Err("addr2line returned non-zero exit".into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut locations = Vec::new();

    // addr2line output format:
    //   0xADDR
    //   function_name
    //   /path/file.rs:line
    let mut lines = stdout.lines();
    while let Some(_addr_line) = lines.next() {
        let _func = lines.next().unwrap_or("??");
        let loc = lines.next().unwrap_or("??:0");
        if loc != "??:0" && !loc.contains('?') {
            if let Some((file, line_str)) = loc.rsplit_once(':') {
                if let Ok(line_num) = line_str.parse::<u64>() {
                    locations.push((file.to_owned(), line_num));
                }
            }
        }
    }

    Ok(locations)
}

#[cfg(target_os = "linux")]
fn count_source_lines(dir: &str) -> Result<usize, String> {
    let mut total = 0usize;
    let mut dirs = vec![dir.to_owned()];
    while let Some(current) = dirs.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue,
            };
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
                // Skip hidden dirs, target, node_modules, etc.
                if !name.starts_with('.') && name != "target" {
                    dirs.push(path.to_str().unwrap_or("").to_owned());
                }
            } else if path.extension().map_or(false, |e| e == "rs") {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for line in content.lines() {
                        let t = line.trim();
                        if !t.is_empty() && !t.starts_with("//") {
                            total += 1;
                        }
                    }
                }
            }
        }
    }

    Ok(total)
}

// ======================================================================
// Helper utilities
// ======================================================================

fn which(name: &str) -> Result<PathBuf, ()> {
    let paths = std::env::var_os("PATH").ok_or(())?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Ok(candidate);
        }
        if cfg!(windows) {
            let candidate_exe = dir.join(format!("{name}.exe"));
            if candidate_exe.exists() {
                return Ok(candidate_exe);
            }
        }
    }
    Err(())
}

fn parse_test_binaries(json_output: &[u8]) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(json_output);
    let mut binaries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        if let Ok(artifact) = serde_json::from_str::<CargoArtifact>(line)
            && artifact.reason == "compiler-artifact"
        {
            for filename in &artifact.filenames {
                let path = PathBuf::from(filename);
                if path.is_file() {
                    binaries.push(path);
                }
            }
        }
    }
    binaries
}

#[derive(serde::Deserialize)]
struct CargoArtifact {
    reason: String,
    filenames: Vec<String>,
}

fn glob_dir(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>, String> {
    let mut results = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {dir:?}: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("entry: {e}"))?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(OsStr::to_str)
            && name.contains(pattern.trim_end_matches('*'))
        {
            results.push(path);
        }
    }
    Ok(results)
}

fn report_filename(format: CoverageFormat) -> String {
    match format {
        CoverageFormat::Summary => "summary.txt".into(),
        CoverageFormat::Html => "index.html".into(),
        CoverageFormat::Lcov => "lcov.info".into(),
        CoverageFormat::Json => "coverage.json".into(),
        CoverageFormat::Cobertura => "cobertura.xml".into(),
    }
}

fn parse_coverage_percentages(summary: &str) -> (f64, f64, f64) {
    let mut line = 0.0;
    let mut func = 0.0;
    let mut region = 0.0;
    for line_text in summary.lines() {
        let t = line_text.trim();
        if t.starts_with("Lines:") || t.starts_with("  Lines:") {
            line = extract_pct(t);
        } else if t.starts_with("Functions:") || t.starts_with("  Functions:") {
            func = extract_pct(t);
        } else if t.starts_with("Regions:") || t.starts_with("  Regions:") {
            region = extract_pct(t);
        }
    }
    (line, func, region)
}

fn extract_pct(s: &str) -> f64 {
    if let Some(start) = s.find(|c: char| c.is_ascii_digit()) {
        let rest = &s[start..];
        if let Some(end) = rest.find('%')
            && let Ok(val) = rest[..end].parse::<f64>()
        {
            return val;
        }
    }
    0.0
}

#[cfg(target_os = "linux")]
fn open_in_browser(path: &Path) {
    let _ = Command::new("xdg-open").arg(path).status();
}

#[cfg(target_os = "macos")]
fn open_in_browser(path: &Path) {
    let _ = Command::new("open").arg(path).status();
}

#[cfg(target_os = "windows")]
fn open_in_browser(path: &Path) {
    let _ = Command::new("cmd").args(["/c", "start", ""]).arg(path).status();
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn open_in_browser(_path: &Path) {
    eprintln!("--open not supported on this platform");
}
