//! A [`Clone`] bound was introduced to [`http::Extensions`] as of http 1.x, and [`SlotGuard`] is
//! not [`Clone`], meaning we need to wrap SlotGuard<T> metrics with Arc Mutex before insertion
//!
//! To expose the types directly, we would need non-cloneable [`http::Extensions`] or a custom
//! cloneable [`SlotGuard`] implementation to expose the types directly

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::sync::PoisonError;

use metrique::OnParentDrop;
use metrique::Slot;
use metrique::SlotGuard;
use thiserror::Error;

use crate::traits::ThreadSafeCloseEntry;

/// Wraps a [`metrique::CloseEntry`], T, with [`Arc<Mutex<SlotGuard<T>>>`] to be Clone and thread safe
/// for insertion into [`http::Extensions`]
///
/// Example
///
/// ```rust
/// pub async fn get_pokemon_species(
///     input: input::GetPokemonSpeciesInput,
///     state: Extension<Arc<State>>,
///     Extension(metrics): Extension<Metrics<PokemonOperationMetrics>>,
/// ) -> Result<output::GetPokemonSpeciesOutput, error::GetPokemonSpeciesError> {
///     metrics.set(|mut metrics| {
///         metrics.get_pokemon_species_metrics = Some("hello world".to_string());
///     });
/// }
/// ```
#[derive(Clone)]
pub struct Metrics<T>
where
    T: ThreadSafeCloseEntry,
{
    inner: Arc<Mutex<SlotGuard<T>>>,
}
impl<T> Metrics<T>
where
    T: ThreadSafeCloseEntry,
{
    /// Intended to be used by `aws_smithy_http_metrics_macro` only
    ///
    /// Takes a [`SlotGuard<T>`] to construct an instance of this wrapper type
    #[doc(hidden)]
    pub fn __macro_new(slotguard: SlotGuard<T>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(slotguard)),
        }
    }

    /// Set the metrics
    ///
    /// Example
    ///
    /// ```rust
    /// metrics.set(|mut metrics| {
    ///     metrics.get_pokemon_species_metrics = Some("hello world".to_string());
    /// });
    /// ```
    pub fn set<F>(&self, f: F) -> Result<(), MetricsError>
    where
        F: FnOnce(MutexGuard<'_, SlotGuard<T>>),
    {
        let metrics = self.inner.lock()?;
        (f)(metrics);

        Ok(())
    }
}

/// A type of error which can be returned when attempting to set metrics.
#[derive(Error, Debug)]
pub enum MetricsError {
    #[error("Mutex was poisoned")]
    PoisonError,
}
impl<T> From<PoisonError<MutexGuard<'_, SlotGuard<T>>>> for MetricsError
where
    T: ThreadSafeCloseEntry,
{
    fn from(_: PoisonError<MutexGuard<'_, SlotGuard<T>>>) -> Self {
        MetricsError::PoisonError
    }
}
