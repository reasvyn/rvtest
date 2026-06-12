use std::fmt::Debug;
use std::time::Instant;

use crate::core::{TestCase, TestStatus};

/// Run a test against multiple input values, producing one [`TestCase`] per
/// input. Each case is automatically named `"{name}[{index}]"`.
///
/// The `test` closure receives a reference to each input and should use
/// `assert!` / `assert_eq!` to validate behaviour. Panics are caught and
/// reported as failures on the individual case.
///
/// # Example
///
/// ```ignore
/// use rvtest::param::parametrize;
///
/// let cases = parametrize("addition", vec![
///     (1, 2, 3),
///     (0, 0, 0),
///     (-1, 1, 0),
///     (-1, -2, -3),
/// ], |(a, b, expected)| {
///     assert_eq!(a + b, *expected);
/// });
///
/// assert_eq!(cases.len(), 4);
/// assert!(cases.iter().all(|c| c.status.is_passed()));
/// ```
pub fn parametrize<Input, Test>(
    name: &str,
    cases: impl IntoIterator<Item = Input>,
    test: Test,
) -> Vec<TestCase>
where
    Input: Debug,
    Test: Fn(&Input),
{
    let mut results = Vec::new();

    for (index, input) in cases.into_iter().enumerate() {
        let test_name = format!("{}[{}]", name, index);
        let start = Instant::now();

        let status = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            test(&input);
        }));

        let duration = start.elapsed();
        let status = match status {
            Ok(_) => TestStatus::Passed,
            Err(panic_info) => {
                let reason = extract_panic_message(&panic_info);
                TestStatus::Failed { reason, location: None }
            }
        };

        results.push(TestCase {
            name: test_name,
            suite: Some(name.to_owned()),
            tags: Vec::new(),
            status,
            duration,
            assertions: 0,
            location: None,
            parameters: vec![("input".to_owned(), format!("{input:?}"))],
        });
    }

    results
}

/// Run a test against named input values, producing one [`TestCase`] per
/// input with the given label.
///
/// # Example
///
/// ```ignore
/// use rvtest::param::parametrize_named;
///
/// let cases = parametrize_named("parse", vec![
///     ("empty", ""),
///     ("valid", "42"),
///     ("negative", "-1"),
/// ], |input| {
///     assert!(input.parse::<i32>().is_ok() || input.is_empty());
/// });
/// ```
pub fn parametrize_named<'a, Input, Test>(
    suite_name: &str,
    cases: impl IntoIterator<Item = (&'a str, Input)>,
    test: Test,
) -> Vec<TestCase>
where
    Input: Debug,
    Test: Fn(&Input),
{
    let mut results = Vec::new();

    for (label, input) in cases.into_iter() {
        let test_name = format!("{} :: {}", suite_name, label);
        let start = Instant::now();

        let status = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            test(&input);
        }));

        let duration = start.elapsed();
        let status = match status {
            Ok(_) => TestStatus::Passed,
            Err(panic_info) => {
                let reason = extract_panic_message(&panic_info);
                TestStatus::Failed { reason, location: None }
            }
        };

        results.push(TestCase {
            name: test_name,
            suite: Some(suite_name.to_owned()),
            tags: Vec::new(),
            status,
            duration,
            assertions: 0,
            location: None,
            parameters: vec![("input".to_owned(), format!("{input:?}"))],
        });
    }

    results
}

fn extract_panic_message(panic_info: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic_info.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_info.downcast_ref::<String>() {
        s.clone()
    } else {
        "test panicked".to_owned()
    }
}
