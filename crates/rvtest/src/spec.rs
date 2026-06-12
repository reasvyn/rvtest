use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::core::{RunnerConfig, SourceLocation, TestCase, TestStatus, TestSuite};

/// Captures the current source location at macro expansion time.
macro_rules! source_location {
    () => {
        Some(SourceLocation {
            file: file!().to_owned(),
            line: line!(),
            column: Some(column!()),
        })
    };
}

/// A BDD-style test specification builder.
///
/// `Spec` lets you organise tests in a nested, descriptive hierarchy using
/// [`describe`] / [`it`](Spec::it) blocks, attach metadata such as
/// [`tags`](Spec::tag), [`timeout`](Spec::timeout) and [`retries`](Spec::retries),
/// and register lifecycle hooks via [`before_all`](Spec::before_all) /
/// [`after_all`](Spec::after_all).
///
/// # Example inside `#[test]` (recommended)
///
/// ```ignore
/// use rvtest::spec::describe;
///
/// #[test]
/// fn my_tests() {
///     describe("Calculator")
///         .describe("addition")
///             .it("adds positive numbers", || {
///                 assert_eq!(2 + 2, 4);
///             })
///             .tag("arithmetic")
///             .timeout(std::time::Duration::from_secs(2))
///         .run()
///         .assert_all_pass();
/// }
/// ```
///
/// Call [`run`](Spec::run) to execute all leaf tests and produce a [`TestSuite`],
/// then call [`assert_all_pass`](crate::core::TestSuite::assert_all_pass) to
/// verify results inside a `#[test]`.
pub struct Spec {
    name: String,
    description: Option<String>,
    tags: Vec<String>,
    setup: Option<Arc<dyn Fn() + Send + Sync>>,
    teardown: Option<Arc<dyn Fn() + Send + Sync>>,
    children: Vec<Spec>,
    tests: Vec<TestEntry>,
    timeout: Option<Duration>,
    retries: u32,
}

struct TestEntry {
    name: String,
    location: Option<SourceLocation>,
    test_fn: Arc<dyn Fn() + Send + Sync>,
}

/// Create a new top-level `Spec` with the given name.
///
/// This is the entry point for BDD-style test organisation. Use chained
/// method calls to describe the expected behaviour, then call [`run`](Spec::run).
pub fn describe(name: &str) -> Spec {
    Spec::new(name)
}

impl Spec {
    /// Create a new `Spec` with the given name.
    pub fn new(name: &str) -> Self {
        Spec {
            name: name.to_owned(),
            description: None,
            tags: Vec::new(),
            setup: None,
            teardown: None,
            children: Vec::new(),
            tests: Vec::new(),
            timeout: None,
            retries: 0,
        }
    }

    /// Attach a description to this spec block.
    pub fn description(mut self, text: &str) -> Self {
        self.description = Some(text.to_owned());
        self
    }

    /// Add a tag to this spec and all contained tests.
    pub fn tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_owned());
        self
    }

    /// Set the default timeout for tests in this block.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = Some(duration);
        self
    }

    /// Set the number of retries for flaky tests in this block.
    pub fn retries(mut self, count: u32) -> Self {
        self.retries = count;
        self
    }

    /// Register a setup hook run once before any test in this block.
    pub fn before_all(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.setup = Some(Arc::new(hook));
        self
    }

    /// Register a teardown hook run once after all tests in this block.
    pub fn after_all(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.teardown = Some(Arc::new(hook));
        self
    }

    /// Register a leaf test case with the given name and body.
    ///
    /// The test body should use `assert!` / `assert_eq!` and will have its
    /// panics caught and reported as failures.
    pub fn it(mut self, name: &str, test: impl Fn() + Send + Sync + 'static) -> Self {
        self.tests.push(TestEntry {
            name: name.to_owned(),
            location: source_location!(),
            test_fn: Arc::new(test),
        });
        self
    }

    /// Nest a child spec block inside this one.
    ///
    /// Child specs inherit the parent's tags, timeout, and retry settings
    /// unless they override them.
    pub fn describe(mut self, name: &str) -> SpecBuilder {
        let child_index = self.children.len();
        let child = Spec::new(name);
        self.children.push(child);
        SpecBuilder { parent: self, path: vec![child_index] }
    }

    fn collect_tests(
        &self,
        prefix: &str,
        inherited_tags: &[String],
        inherited_timeout: Option<Duration>,
        inherited_retries: u32,
        config: &RunnerConfig,
    ) -> Vec<CollectedTest> {
        let full_name = if prefix.is_empty() {
            self.name.clone()
        } else {
            format!("{} :: {}", prefix, self.name)
        };

        let merged_tags: Vec<String> = inherited_tags
            .iter()
            .cloned()
            .chain(self.tags.iter().cloned())
            .collect();

        let merged_timeout = self.timeout.or(inherited_timeout).or(config.default_timeout);
        let merged_retries = if self.retries > 0 { self.retries } else { inherited_retries.max(config.default_retries) };

        let mut collected = Vec::new();

        for entry in &self.tests {
            collected.push(CollectedTest {
                full_name: format!("{} :: {}", full_name, entry.name),
                suite_name: Some(full_name.clone()),
                tags: merged_tags.clone(),
                timeout: merged_timeout,
                retries: merged_retries,
                test_fn: entry.test_fn.clone(),
                location: entry.location.clone(),
                parameters: Vec::new(),
            });
        }

        for child in &self.children {
            collected.extend(child.collect_tests(
                &full_name,
                &merged_tags,
                merged_timeout,
                merged_retries,
                config,
            ));
        }

        collected
    }

    /// Execute all leaf tests in this spec tree and return a `TestSuite`.
    ///
    /// Hooks (`before_all` / `after_all`) are honoured per block. Timing
    /// information is collected for each test and for the suite as a whole.
    pub fn run(self) -> TestSuite {
        let config = RunnerConfig::default();
        self.run_with_config(&config)
    }

    /// Execute tests with an explicit [`RunnerConfig`].
    pub fn run_with_config(self, config: &RunnerConfig) -> TestSuite {
        let mut suite = TestSuite::new(&self.name);
        suite.description = self.description.clone();

        let collected = self.collect_tests("", &[], None, 0, config);

        // Apply tag and name filtering.
        let filtered: Vec<_> = collected
            .into_iter()
            .filter(|t| {
                crate::tag::tags_match(&t.tags, config)
                    && crate::tag::name_matches(&t.full_name, config.filter.as_deref())
            })
            .collect();

        let start = Instant::now();

        // Run setup hook if present.
        if let Some(ref setup) = self.setup {
            setup();
        }

        let test_cases = if config.parallel && filtered.len() > 1 {
            run_parallel(&filtered, config)
        } else {
            run_sequential(&filtered, config)
        };

        // Run teardown hook if present.
        if let Some(ref teardown) = self.teardown {
            teardown();
        }

        suite.duration = start.elapsed();
        suite.tests = test_cases;
        suite
    }
}

/// A builder that lets you chain `.describe()` calls on a parent spec.
///
/// Created by [`Spec::describe`], this wrapper holds a reference to the
/// parent and allows you to chain configuration on the child before
/// returning to the parent.
pub struct SpecBuilder {
    parent: Spec,
    /// Path of indices from `parent` to the current child.
    /// Empty = at parent level (never happens); `[0]` = first child of parent.
    path: Vec<usize>,
}

impl SpecBuilder {
    fn child_mut(&mut self) -> &mut Spec {
        let mut current = &mut self.parent;
        for &idx in &self.path {
            current = &mut current.children[idx];
        }
        current
    }

    /// Attach a description to the child spec.
    pub fn description(mut self, text: &str) -> Self {
        self.child_mut().description = Some(text.to_owned());
        self
    }

    /// Add a tag to the child spec.
    pub fn tag(mut self, tag: &str) -> Self {
        self.child_mut().tags.push(tag.to_owned());
        self
    }

    /// Set a timeout on the child spec.
    pub fn timeout(mut self, duration: Duration) -> Self {
        self.child_mut().timeout = Some(duration);
        self
    }

    /// Set retries on the child spec.
    pub fn retries(mut self, count: u32) -> Self {
        self.child_mut().retries = count;
        self
    }

    /// Register a setup hook on the child spec.
    pub fn before_all(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.child_mut().setup = Some(Arc::new(hook));
        self
    }

    /// Register a teardown hook on the child spec.
    pub fn after_all(mut self, hook: impl Fn() + Send + Sync + 'static) -> Self {
        self.child_mut().teardown = Some(Arc::new(hook));
        self
    }

    /// Add a leaf test to the child spec.
    pub fn it(mut self, name: &str, test: impl Fn() + Send + Sync + 'static) -> Self {
        self.child_mut().tests.push(TestEntry {
            name: name.to_owned(),
            location: source_location!(),
            test_fn: Arc::new(test),
        });
        self
    }

    /// Nest a deeper spec inside the child.
    pub fn describe(mut self, name: &str) -> SpecBuilder {
        let child = Spec::new(name);
        self.child_mut().children.push(child);
        let child_index = self.child_mut().children.len() - 1;
        let mut path = self.path;
        path.push(child_index);
        SpecBuilder { parent: self.parent, path }
    }

    /// Run all tests starting from the parent spec.
    pub fn run(self) -> TestSuite {
        self.parent.run()
    }

    /// Run all tests with an explicit config.
    pub fn run_with_config(self, config: &RunnerConfig) -> TestSuite {
        self.parent.run_with_config(config)
    }
}

// ---------------------------------------------------------------------------
// Execution helpers
// ---------------------------------------------------------------------------

struct CollectedTest {
    full_name: String,
    suite_name: Option<String>,
    tags: Vec<String>,
    timeout: Option<Duration>,
    retries: u32,
    test_fn: Arc<dyn Fn() + Send + Sync>,
    location: Option<SourceLocation>,
    parameters: Vec<(String, String)>,
}

// Manual Clone impl because Arc<dyn Fn() + Send + Sync> requires it via Arc.
impl Clone for CollectedTest {
    fn clone(&self) -> Self {
        CollectedTest {
            full_name: self.full_name.clone(),
            suite_name: self.suite_name.clone(),
            tags: self.tags.clone(),
            timeout: self.timeout,
            retries: self.retries,
            test_fn: Arc::clone(&self.test_fn),
            location: self.location.clone(),
            parameters: self.parameters.clone(),
        }
    }
}

fn run_sequential(tests: &[CollectedTest], config: &RunnerConfig) -> Vec<TestCase> {
    let mut results = Vec::new();
    for t in tests {
        let case = execute_test(t);
        let should_stop = config.fail_fast && case.status.is_failed();
        results.push(case);
        if should_stop {
            break;
        }
    }
    results
}

fn run_parallel(tests: &[CollectedTest], config: &RunnerConfig) -> Vec<TestCase> {
    let max_threads = config.max_threads.min(tests.len());
    let chunk_size = tests.len().div_ceil(max_threads);

    let mut handles = Vec::new();

    for chunk in tests.chunks(chunk_size) {
        let chunk: Vec<CollectedTest> = chunk.to_vec(); // Clone for the thread.
        handles.push(std::thread::spawn(move || {
            chunk.into_iter().map(|t| execute_test(&t)).collect::<Vec<_>>()
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        if let Ok(mut chunk_results) = handle.join() {
            results.append(&mut chunk_results);
        }
    }

    results
}

fn execute_test(t: &CollectedTest) -> TestCase {
    let start = Instant::now();

    let status = match t.timeout {
        Some(timeout) => run_with_timeout(&t.test_fn, timeout, t.retries),
        None => run_with_retry(&t.test_fn, t.retries),
    };

    let duration = start.elapsed();

    TestCase {
        name: t.full_name.clone(),
        suite: t.suite_name.clone(),
        tags: t.tags.clone(),
        status,
        duration,
        assertions: 0,
        location: t.location.clone(),
        parameters: t.parameters.clone(),
    }
}

fn run_with_retry(test: &Arc<dyn Fn() + Send + Sync>, retries: u32) -> TestStatus {
    let max_attempts = retries.saturating_add(1);

    for attempt in 1..=max_attempts {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (test)();
        }));

        match result {
            Ok(_) => return TestStatus::Passed,
            Err(panic_info) => {
                if attempt == max_attempts {
                    let reason = extract_panic_message(&panic_info);
                    return TestStatus::Failed { reason, location: None };
                }
            }
        }
    }

    TestStatus::Failed {
        reason: "exhausted retries".to_owned(),
        location: None,
    }
}

fn run_with_timeout(
    test: &Arc<dyn Fn() + Send + Sync>,
    timeout: Duration,
    retries: u32,
) -> TestStatus {
    let test = Arc::clone(test);

    let (tx, rx) = std::sync::mpsc::channel();

    let _handle = std::thread::spawn(move || {
        let status = run_with_retry(&test, retries);
        let _ = tx.send(status);
    });

    match rx.recv_timeout(timeout) {
        Ok(status) => status,
        Err(_) => TestStatus::TimedOut { duration: timeout, location: None },
    }
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
