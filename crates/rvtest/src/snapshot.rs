//! Snapshot testing — file-based assertions with automatic review.
//!
//! ```ignore
//! use rvtest::snapshot::assert_snapshot;
//!
//! #[test]
//! fn json_output() {
//!     let data = serde_json::to_string_pretty(&my_struct()).unwrap();
//!     assert_snapshot!("json_output", &data);
//! }
//! ```

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// Global configuration (set by CLI)
// ---------------------------------------------------------------------------

static UPDATE_ALL: AtomicBool = AtomicBool::new(false);
static SNAPSHOT_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Enable global "update all snapshots" mode.
/// When set, any snapshot mismatch will overwrite the snapshot file instead
/// of failing.
pub fn set_update_all(enabled: bool) {
    UPDATE_ALL.store(enabled, Ordering::SeqCst);
}

/// Returns `true` if snapshot update mode is active.
pub fn is_update_all() -> bool {
    UPDATE_ALL.load(Ordering::SeqCst)
}

// ---------------------------------------------------------------------------
// Snapshot directory resolution
// ---------------------------------------------------------------------------

fn snapshot_dir() -> PathBuf {
    let mut guard = SNAPSHOT_DIR.lock().unwrap();
    if let Some(dir) = guard.clone() {
        return dir;
    }
    let candidates = [
        PathBuf::from(".snapshots"),
        PathBuf::from("tests/.snapshots"),
    ];
    for c in &candidates {
        if c.exists() {
            *guard = Some(c.clone());
            return c.to_path_buf();
        }
    }
    // Default: create .snapshots in project root.
    *guard = Some(PathBuf::from(".snapshots"));
    PathBuf::from(".snapshots")
}

/// Override the snapshot directory (for testing or custom layouts).
pub fn set_snapshot_dir(path: impl Into<PathBuf>) {
    let dir = path.into();
    let _ = std::fs::create_dir_all(&dir);
    let mut guard = SNAPSHOT_DIR.lock().unwrap();
    *guard = Some(dir);
}

// ---------------------------------------------------------------------------
// Core assertion function
// ---------------------------------------------------------------------------

/// Assert that `value` matches the stored snapshot identified by `name`.
///
/// On first run (no snapshot file), the snapshot is created and the test
/// fails, prompting a review.  On subsequent runs, the value is compared
/// to the stored snapshot.  On mismatch:
///
/// - If `--update-all` was set: the snapshot is overwritten silently.
/// - Otherwise: the test panics with a diff.
pub fn assert_snapshot(name: &str, value: &dyn fmt::Display) {
    let result = assert_snapshot_impl(name, value, &snapshot_dir());
    if let Err(msg) = result {
        panic!("{}", msg);
    }
}

/// Same as `assert_snapshot` but in a custom directory.
pub fn assert_snapshot_in(name: &str, value: &dyn fmt::Display, dir: &Path) {
    let result = assert_snapshot_impl(name, value, dir);
    if let Err(msg) = result {
        panic!("{}", msg);
    }
}

fn assert_snapshot_impl(name: &str, value: &dyn fmt::Display, dir: &Path) -> Result<(), String> {
    // Sanitise name for filesystem.
    let safe_name: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();
    let snap_path = dir.join(format!("{}.snap", safe_name));

    let rendered = value.to_string();

    // If snapshot doesn't exist, create it.
    if !snap_path.exists() {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("mkdir {:?}: {e}", dir))?;
        std::fs::write(&snap_path, &rendered)
            .map_err(|e| format!("write {:?}: {e}", snap_path))?;

        if is_update_all() {
            return Ok(());
        }
        return Err(format!(
            "snapshot `{}` created at {:?}.\n\
             Review the content and commit the snapshot file.\n\
             Use `--update-all` to auto-accept new snapshots.",
            name, snap_path
        ));
    }

    // Read existing snapshot.
    let existing = std::fs::read_to_string(&snap_path)
        .map_err(|e| format!("read {:?}: {e}", snap_path))?;

    if existing == rendered {
        return Ok(());
    }

    // Mismatch.
    if is_update_all() {
        std::fs::write(&snap_path, &rendered)
            .map_err(|e| format!("write {:?}: {e}", snap_path))?;
        return Ok(());
    }

    // Generate diff.
    let diff = simple_diff(&existing, &rendered, &snap_path);
    Err(format!(
        "snapshot `{}` mismatch!\n\
         expected (snapshot)\n\
         actual (new)\n\
         {}\n\
         Rerun with `--update-all` to accept the new snapshot.",
        name, diff
    ))
}

// ---------------------------------------------------------------------------
// Simple line-diff generator (no external dependencies)
// ---------------------------------------------------------------------------

fn simple_diff(old: &str, new: &str, path: &Path) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = String::new();

    let max = old_lines.len().max(new_lines.len());
    for i in 0..max {
        let old_line = old_lines.get(i).copied().unwrap_or("");
        let new_line = new_lines.get(i).copied().unwrap_or("");
        if old_line != new_line {
            out.push_str(&format!(
                "  {} | {}\n  {} | {}\n",
                path.display(),
                old_line,
                path.display(),
                new_line,
            ));
        }
    }

    if out.is_empty() {
        // Different lengths but same content up to min — show tail.
        out.push_str(&format!(
            "  {}: snapshot has {} lines, actual has {} lines\n",
            path.display(),
            old_lines.len(),
            new_lines.len(),
        ));
    }

    out
}
