use std::fmt::Debug;
use std::marker::PhantomData;

use rand::rngs::StdRng;
use rand::{Rng, RngCore, SeedableRng};

const DEFAULT_NUM_TESTS: u64 = 100;
const DEFAULT_SHRINKS: u64 = 1000;

/// A strategy for generating random values of type `T`.
///
/// Implement this trait to define how values are produced and optionally
/// shrunk when a failing counterexample is found.
///
/// # Shrinking
///
/// Shrinking tries to simplify a counterexample to its minimal form,
/// making failures easier to diagnose. The default implementation returns
/// an empty vector (no shrinking).
pub trait Strategy<T>: Send + Sync {
    /// Generate a random value of type `T`.
    fn generate(&self, rng: &mut dyn RngCore) -> T;

    /// Produce a list of simpler candidates from a given value, used for
    /// shrinking counterexamples.
    fn shrink(&self, _value: &T) -> Vec<T> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Built-in strategies for common types
// ---------------------------------------------------------------------------

macro_rules! impl_range_strategy {
    ($ty:ty, $range:expr) => {
        impl Strategy<$ty> for RangeStrategy<$ty> {
            fn generate(&self, rng: &mut dyn RngCore) -> $ty {
                rng.gen_range($range)
            }
        }
    };
}

/// Strategy that produces values in a given range.
#[derive(Debug, Clone)]
pub struct RangeStrategy<T> {
    _marker: PhantomData<T>,
}

impl_range_strategy!(i8, i8::MIN..=i8::MAX);
impl_range_strategy!(i16, i16::MIN..=i16::MAX);
impl_range_strategy!(i32, i32::MIN..=i32::MAX);
impl_range_strategy!(i64, i64::MIN..=i64::MAX);
impl_range_strategy!(u8, u8::MIN..=u8::MAX);
impl_range_strategy!(u16, u16::MIN..=u16::MAX);
impl_range_strategy!(u32, u32::MIN..=u32::MAX);
impl_range_strategy!(u64, u64::MIN..=u64::MAX);
impl_range_strategy!(usize, usize::MIN..=usize::MAX);

impl Strategy<bool> for RangeStrategy<bool> {
    fn generate(&self, rng: &mut dyn RngCore) -> bool {
        rng.gen_bool(0.5)
    }

    fn shrink(&self, value: &bool) -> Vec<bool> {
        if *value { vec![false] } else { vec![] }
    }
}

/// Return a strategy for any value of type `T`.
///
/// Supported types: all standard integer types, `bool`, and types composed
/// via combinators.
///
/// # Example
///
/// ```ignore
/// use rutest::property::{check, any};
///
/// check("addition is commutative", any::<i32>(), |v: &i32| true);
/// ```
pub fn any<T: StrategyProvider>() -> T::Strategy {
    T::strategy()
}

/// Trait mapping a type to its default `Strategy`.
///
/// Types that implement [`StrategyProvider`] can be used with [`any`] to
/// obtain a default strategy for property-based testing.
pub trait StrategyProvider: Sized {
    /// The strategy type used to generate values of `Self`.
    type Strategy: Strategy<Self> + Default + Send + Sync;

    /// Returns the default strategy for this type.
    fn strategy() -> Self::Strategy {
        Self::Strategy::default()
    }
}

macro_rules! impl_provider {
    ($ty:ty) => {
        impl StrategyProvider for $ty {
            type Strategy = RangeStrategy<$ty>;

            fn strategy() -> Self::Strategy {
                RangeStrategy { _marker: PhantomData }
            }
        }

        impl Default for RangeStrategy<$ty> {
            fn default() -> Self {
                RangeStrategy { _marker: PhantomData }
            }
        }
    };
}

impl_provider!(i8);
impl_provider!(i16);
impl_provider!(i32);
impl_provider!(i64);
impl_provider!(u8);
impl_provider!(u16);
impl_provider!(u32);
impl_provider!(u64);
impl_provider!(usize);
impl_provider!(bool);

// ---------------------------------------------------------------------------
// Vec strategy
// ---------------------------------------------------------------------------

/// Strategy for generating `Vec<T>` values.
#[derive(Debug, Clone)]
pub struct VecStrategy<S> {
    element_strategy: S,
    min_len: usize,
    max_len: usize,
}

impl<T, S> Strategy<Vec<T>> for VecStrategy<S>
where
    S: Strategy<T> + Send + Sync,
    T: Send + Clone,
{
    fn generate(&self, rng: &mut dyn RngCore) -> Vec<T> {
        let len = rng.gen_range(self.min_len..=self.max_len);
        (0..len).map(|_| self.element_strategy.generate(rng)).collect()
    }

    fn shrink(&self, value: &Vec<T>) -> Vec<Vec<T>> {
        let mut candidates = Vec::new();
        if !value.is_empty() {
            let mut smaller = (*value).clone();
            smaller.pop();
            candidates.push(smaller);
        }
        candidates
    }
}

/// Create a strategy that generates `Vec<T>`s with elements from `strategy`.
///
/// The vector length ranges from `min_len` to `max_len` (inclusive).
pub fn vec<T, S>(strategy: S, min_len: usize, max_len: usize) -> VecStrategy<S>
where
    S: Strategy<T>,
{
    VecStrategy { element_strategy: strategy, min_len, max_len }
}

// ---------------------------------------------------------------------------
// Mapping combinator
// ---------------------------------------------------------------------------

/// A strategy that transforms generated values with a mapping function.
pub struct MapStrategy<S, F, T, U> {
    inner: S,
    f: F,
    _phantom: PhantomData<(T, U)>,
}

impl<S: Clone, F: Clone, T, U> Clone for MapStrategy<S, F, T, U> {
    fn clone(&self) -> Self {
        MapStrategy {
            inner: self.inner.clone(),
            f: self.f.clone(),
            _phantom: PhantomData,
        }
    }
}

impl<S: Debug, F, T, U> Debug for MapStrategy<S, F, T, U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MapStrategy").field("inner", &self.inner).finish()
    }
}

impl<T, U, S, F> Strategy<U> for MapStrategy<S, F, T, U>
where
    S: Strategy<T>,
    F: Fn(T) -> U + Send + Sync,
    T: Send + Sync,
    U: Send + Sync,
{
    fn generate(&self, rng: &mut dyn RngCore) -> U {
        (self.f)(self.inner.generate(rng))
    }

    fn shrink(&self, value: &U) -> Vec<U> {
        // Mapping makes shrinking complex; skip for now.
        let _ = value;
        Vec::new()
    }
}

/// Transform a strategy's output with a mapping function.
pub fn map<T, U, S, F>(strategy: S, f: F) -> MapStrategy<S, F, T, U>
where
    S: Strategy<T>,
    F: Fn(T) -> U,
{
    MapStrategy { inner: strategy, f, _phantom: PhantomData }
}

// ---------------------------------------------------------------------------
// Filter combinator
// ---------------------------------------------------------------------------

/// A strategy that only produces values satisfying a predicate.
pub struct FilterStrategy<S, P, T> {
    inner: S,
    predicate: P,
    max_attempts: u32,
    _phantom: PhantomData<T>,
}

impl<S: Clone, P: Clone, T> Clone for FilterStrategy<S, P, T> {
    fn clone(&self) -> Self {
        FilterStrategy {
            inner: self.inner.clone(),
            predicate: self.predicate.clone(),
            max_attempts: self.max_attempts,
            _phantom: PhantomData,
        }
    }
}

impl<S: Debug, P, T> Debug for FilterStrategy<S, P, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilterStrategy").field("inner", &self.inner).finish()
    }
}

impl<T, S, P> Strategy<T> for FilterStrategy<S, P, T>
where
    S: Strategy<T>,
    P: Fn(&T) -> bool + Send + Sync,
    T: Send + Sync,
{
    fn generate(&self, rng: &mut dyn RngCore) -> T {
        for _ in 0..self.max_attempts {
            let value = self.inner.generate(rng);
            if (self.predicate)(&value) {
                return value;
            }
        }
        self.inner.generate(rng)
    }

    fn shrink(&self, value: &T) -> Vec<T> {
        self.inner
            .shrink(value)
            .into_iter()
            .filter(|v| (self.predicate)(v))
            .collect()
    }
}

/// Create a strategy that only generates values satisfying a predicate.
pub fn filter<T, S, P>(strategy: S, predicate: P) -> FilterStrategy<S, P, T>
where
    S: Strategy<T>,
    P: Fn(&T) -> bool,
{
    FilterStrategy { inner: strategy, predicate, max_attempts: 100, _phantom: PhantomData }
}

// ---------------------------------------------------------------------------
// Property check
// ---------------------------------------------------------------------------

/// Configuration for property-based test execution.
#[derive(Debug, Clone)]
pub struct PropertyConfig {
    /// How many random test cases to generate.
    pub num_tests: u64,
    /// Maximum number of shrink steps per failing case.
    pub max_shrinks: u64,
    /// Seed for deterministic replay.
    pub seed: Option<u64>,
}

impl Default for PropertyConfig {
    fn default() -> Self {
        PropertyConfig { num_tests: DEFAULT_NUM_TESTS, max_shrinks: DEFAULT_SHRINKS, seed: None }
    }
}

/// Run a property-based test.
///
/// Generates random inputs using the given `strategy`, passing each to
/// `property`. If the property returns `false` for any input, the function
/// attempts to shrink the counterexample and then panics with a descriptive
/// message.
///
/// # Example
///
/// ```ignore
/// use rutest::property::{check, any};
///
/// check("reversal is involutive", any::<Vec<i32>>(), |v: &Vec<i32>| {
///     let rev: Vec<_> = v.iter().rev().copied().collect();
///     let revrev: Vec<_> = rev.iter().rev().copied().collect();
///     revrev == **v
/// });
/// ```
pub fn check<T, S>(
    _name: &str,
    strategy: S,
    property: impl Fn(&T) -> bool,
) where
    T: Debug,
    S: Strategy<T>,
{
    check_with(_name, strategy, property, PropertyConfig::default());
}

/// Run a property-based test with a custom configuration.
///
/// Same as [`check`] but accepts a [`PropertyConfig`] for fine-grained
/// control over the number of tests, shrinking, and seeding.
pub fn check_with<T, S>(
    _name: &str,
    strategy: S,
    property: impl Fn(&T) -> bool,
    config: PropertyConfig,
) where
    T: Debug,
    S: Strategy<T>,
{
    let seed = config.seed.unwrap_or_else(rand::random);
    let mut rng = StdRng::seed_from_u64(seed);

    for _ in 0..config.num_tests {
        let value = strategy.generate(&mut rng);
        if !property(&value) {
            let shrunk = shrink_counterexample(&value, &strategy, &property, config.max_shrinks);
            panic!(
                "property falsified after {} test(s)\n\
                 seed: {seed}\n\
                 counterexample: {value:?}\n\
                 shrunk to: {shrunk:?}",
                config.num_tests,
            );
        }
    }
}

/// Attempt to shrink a counterexample to its minimal form.
fn shrink_counterexample<T, S>(
    value: &T,
    strategy: &S,
    property: &impl Fn(&T) -> bool,
    max_shrinks: u64,
) -> String
where
    T: Debug,
    S: Strategy<T>,
{
    let mut best_repr = format!("{:?}", value);
    let mut candidates = strategy.shrink(value);
    let mut iterations = 0u64;

    while !candidates.is_empty() && iterations < max_shrinks {
        match candidates.into_iter().find(|c| !property(c)) {
            Some(candidate) => {
                best_repr = format!("{:?}", candidate);
                candidates = strategy.shrink(&candidate);
            }
            None => break,
        }
        iterations += 1;
    }

    best_repr
}
