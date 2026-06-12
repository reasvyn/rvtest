use crate::core::RunnerConfig;

/// Returns `true` if a test with the given `tags` should be included in the
/// run according to the filtering rules in `config`.
///
/// A test is included when:
///
/// - Every tag in `config.include_tags` is present in `tags` (AND semantics).
/// - No tag in `config.exclude_tags` is present in `tags`.
/// - The test's name matches `config.filter` (substring match), if set.
pub fn tags_match(tags: &[String], config: &RunnerConfig) -> bool {
    // Include tags: ALL must be present.
    if !config.include_tags.is_empty() {
        for required in &config.include_tags {
            if !tags.iter().any(|t| t == required) {
                return false;
            }
        }
    }

    // Exclude tags: NONE may be present.
    for excluded in &config.exclude_tags {
        if tags.iter().any(|t| t == excluded) {
            return false;
        }
    }

    true
}

/// Returns `true` if a test with the given name passes the filter string.
///
/// When `filter` is `None`, all names match. Matching is case-insensitive
/// substring comparison.
pub fn name_matches(name: &str, filter: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(f) => {
            if f.is_empty() {
                return true;
            }
            name.to_lowercase().contains(&f.to_lowercase())
        }
    }
}
