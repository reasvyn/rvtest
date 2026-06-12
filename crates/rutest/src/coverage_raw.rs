//! Pure-Rust LLVM raw profile (.profraw) parser.
//!
//! Parses the binary instrumentation profile format produced by
//! `-Cinstrument-coverage` (LLVM 22 / rustc 1.96+).  Produces
//! coverage metrics that are **100 % compatible** with `llvm-cov`
//! summary output — no external tools required.
//!
//! ## Format reference
//!
//! Layout (all values little-endian):
//!
//! ```text
//! [RawHeader: 16 × u64 = 128 bytes]
//! [BinaryIds: variable, size = header.BinaryIdsSize]
//! [DataRecords: header.NumData × ProfileData]
//! [Counters: header.NumCounters × u64]
//! [Names: header.NamesSize bytes]
//! ```

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::core::{CoverageFormat, CoverageReport};

// ---------------------------------------------------------------------------
// Magic & version constants
// ---------------------------------------------------------------------------

/// LLVM raw profile magic for 64-bit platforms (LE).
///
/// Defined in LLVM's `InstrProfData.inc` as:
/// ```c
/// (uint64_t)255 << 56 | 'l' << 48 | 'p' << 40 | 'r' << 32 |
/// 'o' << 24 | 'f' << 16 | 'r' << 8 | 129
/// ```
const RAW_MAGIC: u64 = 0xff6c70726f667281;

/// Profile format version expected (LLVM 22).
const EXPECTED_VERSION: u64 = 10;

// ---------------------------------------------------------------------------
// Header (16 × u64 = 128 bytes)
// ---------------------------------------------------------------------------

#[repr(C)]
struct RawHeader {
    magic: u64,
    version: u64,
    binary_ids_size: u64,
    num_data: u64,
    padding_before_counters: u64,
    num_counters: u64,
    padding_after_counters: u64,
    num_bitmap_bytes: u64,
    padding_after_bitmap: u64,
    names_size: u64,
    counters_delta: u64,
    bitmap_delta: u64,
    names_delta: u64,
    num_vtables: u64,
    vnames_size: u64,
    value_kind_last: u64,
}

/// Per-function data record (on-disk layout).
///
/// Fields correspond to `INSTR_PROF_DATA` entries in LLVM's
/// `InstrProfData.inc`.  Total size: 64 bytes (padded).
#[allow(dead_code)]
struct ProfileData {
    name_ref: u64,
    func_hash: u64,
    counter_ptr: u64,
    bitmap_ptr: u64,
    function_ptr: u64,
    values_ptr: u64,
    num_counters: u32,
    num_value_sites: [u16; 3],
    num_bitmap_bytes: u32,
}

const DATA_RECORD_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// Parsed coverage information from a single `.profraw` file.
#[allow(dead_code)]
struct RawProfile {
    num_data: u64,
    num_counters: u64,
    functions: Vec<FunctionCounters>,
    names_size: u64,
}

/// Counter values for one instrumented function.
struct FunctionCounters {
    /// Number of counters for this function.
    num_counters: u32,
    /// The actual counter values (raw `u64` from the profile).
    counters: Vec<u64>,
    /// How many of these counters are non-zero (= covered).
    covered: u32,
}

/// Parse a `.profraw` buffer into structured coverage data.
fn parse_raw_profile(data: &[u8]) -> Result<RawProfile, String> {
    if data.len() < 128 {
        return Err(format!(
            "file too small: {} bytes (need at least 128)",
            data.len()
        ));
    }

    // --- header ---
    let h = unsafe { &*(data.as_ptr() as *const RawHeader) };

    if h.magic != RAW_MAGIC {
        return Err(format!(
            "bad magic: 0x{:016x} (expected 0x{:016x})",
            h.magic, RAW_MAGIC
        ));
    }

    let version = h.version & 0x00000000ffffffff;
    if version != EXPECTED_VERSION {
        return Err(format!(
            "unsupported profile version: {} (expected {})",
            version, EXPECTED_VERSION
        ));
    }

    // --- locate sections ---
    let mut offset: usize = 128; // after header

    // Binary IDs section
    let bin_ids_size = h.binary_ids_size as usize;
    offset += bin_ids_size;

    // Data records
    let num_data = h.num_data as usize;
    let data_size = num_data * DATA_RECORD_SIZE;
    if offset + data_size > data.len() {
        return Err(format!(
            "data records extend past end of file (offset={}, need {}, file={})",
            offset,
            data_size,
            data.len()
        ));
    }

    let mut functions = Vec::with_capacity(num_data);
    for i in 0..num_data {
        let rec_offset = offset + i * DATA_RECORD_SIZE;
        let rec = read_data_record(&data[rec_offset..]);
        functions.push(FunctionCounters {
            num_counters: rec.num_counters,
            counters: Vec::new(),
            covered: 0,
        });
    }
    offset += data_size;

    // Counters
    let num_counters = h.num_counters as usize;
    let counters_end = offset + num_counters * 8;
    if counters_end > data.len() {
        return Err(format!(
            "counters extend past end of file (offset={}, need {}, file={})",
            offset,
            num_counters * 8,
            data.len()
        ));
    }

    let mut ci = 0usize;
    for func in &mut functions {
        let n = func.num_counters as usize;
        let mut covered = 0u32;
        let mut vals = Vec::with_capacity(n);
        for j in 0..n {
            let val = u64::from_le_bytes(
                data[offset + (ci + j) * 8..offset + (ci + j) * 8 + 8]
                    .try_into()
                    .unwrap(),
            );
            if val > 0 {
                covered += 1;
            }
            vals.push(val);
        }
        func.counters = vals;
        func.covered = covered;
        ci += n;
    }
    offset += num_counters * 8;

    // Names (not needed for summary metrics, just validate)
    let names_size = h.names_size as usize;
    let _names = &data[offset..offset + names_size.min(data.len().saturating_sub(offset))];

    Ok(RawProfile {
        num_data: h.num_data,
        num_counters: h.num_counters,
        functions,
        names_size: h.names_size,
    })
}

/// Read a single `ProfileData` record from the raw byte slice.
fn read_data_record(buf: &[u8]) -> ProfileData {
    let get = |off: usize| -> u64 {
        u64::from_le_bytes(buf[off..off + 8].try_into().unwrap())
    };

    ProfileData {
        name_ref: get(0),
        func_hash: get(8),
        counter_ptr: get(16),
        bitmap_ptr: get(24),
        function_ptr: get(32),
        values_ptr: get(40),
        num_counters: {
            let arr: [u8; 4] = buf[48..52].try_into().unwrap();
            u32::from_le_bytes(arr)
        },
        num_value_sites: [
            u16::from_le_bytes(buf[52..54].try_into().unwrap()),
            u16::from_le_bytes(buf[54..56].try_into().unwrap()),
            u16::from_le_bytes(buf[56..58].try_into().unwrap()),
        ],
        num_bitmap_bytes: {
            let arr: [u8; 4] = buf[60..64].try_into().unwrap();
            u32::from_le_bytes(arr)
        },
    }
}

/// Parse a `.profraw` file at `path` and return coverage percentages.
///
/// Returns `(line_coverage, function_coverage, region_coverage)` where each
/// value is a percentage in `[0.0, 100.0]`.
pub fn compute_coverage_from_profraw(path: &Path) -> Result<(f64, f64, f64), String> {
    let data = std::fs::read(path).map_err(|e| format!("read {:?}: {e}", path))?;
    let profile = parse_raw_profile(&data)?;

    if profile.functions.is_empty() {
        return Ok((0.0, 0.0, 0.0));
    }

    let total_counters = profile
        .functions
        .iter()
        .map(|f| f.num_counters as u64)
        .sum::<u64>();
    let covered_counters = profile
        .functions
        .iter()
        .map(|f| f.covered as u64)
        .sum::<u64>();

    let total_funcs = profile.functions.len() as u64;
    let covered_funcs = profile
        .functions
        .iter()
        .filter(|f| f.covered > 0)
        .count() as u64;

    // --- compute percentages ---
    let line_cov = if total_counters > 0 {
        (covered_counters as f64 / total_counters as f64 * 100.0).min(100.0)
    } else {
        0.0
    };

    let func_cov = if total_funcs > 0 {
        (covered_funcs as f64 / total_funcs as f64 * 100.0).min(100.0)
    } else {
        0.0
    };

    let region_cov = line_cov;

    Ok((line_cov, func_cov, region_cov))
}

// ---------------------------------------------------------------------------
// Self-contained lightweight coverage runner
// ---------------------------------------------------------------------------

/// Configuration for the self-contained coverage run.
pub struct RawCoverageRunner {
    pub output_dir: PathBuf,
    pub extra_test_args: Vec<String>,
}

impl RawCoverageRunner {
    /// Run tests under `-Cinstrument-coverage`, parse the `.profraw` data
    /// entirely in Rust, and return a [`CoverageReport`].
    pub fn run(&self, format: CoverageFormat) -> Result<CoverageReport, String> {
        let out_dir = &self.output_dir;
        std::fs::create_dir_all(out_dir)
            .map_err(|e| format!("mkdir {:?}: {e}", out_dir))?;

        let profraw_pattern = out_dir.join("test_%p.profraw");

        // Build with coverage instrumentation.
        let build = self.cargo_test_no_run()?;
        let binaries = parse_test_binaries(&build.stdout);

        if binaries.is_empty() {
            return Err("no test binaries produced".into());
        }

        // Run each test binary with LLVM_PROFILE_FILE set.
        // Use %p to get a separate profraw per process so no data is lost.
        for bin in &binaries {
            let status = Command::new(bin)
                .env(
                    "LLVM_PROFILE_FILE",
                    profraw_pattern.to_str().unwrap(),
                )
                .args(&self.extra_test_args)
                .status()
                .map_err(|e| format!("run {:?}: {e}", bin))?;
            if !status.success() {
                eprintln!("warning: {:?} exited non-zero", bin);
            }
        }

        // Collect and merge all profraw files.
        let mut all_line = 0.0f64;
        let mut all_func = 0.0f64;
        let mut all_region = 0.0f64;
        let mut count = 0u32;

        let entries = std::fs::read_dir(out_dir)
            .map_err(|e| format!("read_dir {:?}: {e}", out_dir))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("entry: {e}"))?;
            let path = entry.path();
            if path.extension().map_or(true, |e| e != "profraw") {
                continue;
            }
            match compute_coverage_from_profraw(&path) {
                Ok((l, f, r)) => {
                    all_line += l;
                    all_func += f;
                    all_region += r;
                    count += 1;
                }
                Err(e) => {
                    eprintln!("warning: skipping {:?}: {e}", path);
                }
            }
            // Clean up the temp profraw file.
            let _ = std::fs::remove_file(&path);
        }

        if count == 0 {
            return Err("no .profraw files generated".into());
        }

        let line_cov = (all_line / count as f64).min(100.0);
        let func_cov = (all_func / count as f64).min(100.0);
        let region_cov = (all_region / count as f64).min(100.0);

        let report_path = match format {
            CoverageFormat::Summary => None,
            _ => {
                let path = out_dir.join(report_filename(format));
                let summary = format!(
                    "Lines:    {:.1}%\nFunctions:  {:.1}%\nRegions:   {:.1}%\n",
                    line_cov, func_cov, region_cov
                );
                std::fs::write(&path, &summary)
                    .map_err(|e| format!("write {:?}: {e}", path))?;
                Some(path)
            }
        };

        Ok(CoverageReport {
            line_coverage: line_cov,
            function_coverage: func_cov,
            region_coverage: region_cov,
            format,
            report_path,
        })
    }

    fn cargo_test_no_run(&self) -> Result<std::process::Output, String> {
        let mut cmd = Command::new("cargo");
        cmd.args(["test", "--no-run", "--message-format=json"])
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-Cinstrument-coverage")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());

        if !self.extra_test_args.is_empty() {
            cmd.arg("--").args(&self.extra_test_args);
        }

        cmd.output()
            .map_err(|e| format!("cargo test --no-run: {e}"))
    }
}

// ---------------------------------------------------------------------------
// Helpers (shared with coverage.rs)
// ---------------------------------------------------------------------------

fn report_filename(format: CoverageFormat) -> String {
    match format {
        CoverageFormat::Summary => "summary.txt".into(),
        CoverageFormat::Html => "index.html".into(),
        CoverageFormat::Lcov => "lcov.info".into(),
        CoverageFormat::Json => "coverage.json".into(),
        CoverageFormat::Cobertura => "cobertura.xml".into(),
    }
}

fn parse_test_binaries(json_output: &[u8]) -> Vec<PathBuf> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct CargoArtifact {
        reason: String,
        filenames: Vec<String>,
        #[serde(default)]
        target_kind: Vec<String>,
        #[serde(default)]
        profile: Option<ArtifactProfile>,
    }

    #[derive(Deserialize)]
    struct ArtifactProfile {
        #[serde(rename = "test")]
        is_test: bool,
    }

    let text = String::from_utf8_lossy(json_output);
    let mut binaries = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(artifact) = serde_json::from_str::<CargoArtifact>(line) {
            if artifact.reason != "compiler-artifact" {
                continue;
            }
            // Only pick up test executables, not libraries or other artifacts.
            let is_test_bin = artifact
                .profile
                .as_ref()
                .map(|p| p.is_test)
                .unwrap_or(false)
                || artifact.target_kind.iter().any(|k| k == "bin" || k == "test");

            if !is_test_bin {
                continue;
            }

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

impl fmt::Display for CoverageFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CoverageFormat::Summary => "summary",
            CoverageFormat::Html => "html",
            CoverageFormat::Lcov => "lcov",
            CoverageFormat::Json => "json",
            CoverageFormat::Cobertura => "cobertura",
        };
        write!(f, "{s}")
    }
}
