/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! A [`Clone`] bound was introduced to [`http::Extensions`] as of http 1.x, and [`SlotGuard`] is
//! not [`Clone`], meaning we need to wrap `SlotGuard<T>` metrics with Arc Mutex before insertion
//!
//! To expose the types directly, we would need non-cloneable [`http::Extensions`] or a custom
//! cloneable [`SlotGuard`] implementation to expose the types directly
//!
//! Uses a double-slot pattern to enable metrics to be modified in handlers
//! while still being collected and serialized by the metrics layer. [`Metrics`]
//! implements [`FromParts`] to
//!
//! 1. The outer slot (`Slot<Slot<T>>`) lives in the main metrics struct
//! 2. The outer slot is opened, yielding a `SlotGuard<Slot<T>>` stored in request extensions
//! 3. Handlers extract this and open the inner slot to get `SlotGuard<T>`
//! 4. When the handler finishes, the inner guard drops and sends values back to the inner slot
//! 5. When the request finishes, the outer guard drops and sends the inner slot back to the outer slot
//! 6. The metrics layer serializes the outer slot, which now contains the handler's values

use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::Mutex;

use metrique::OnParentDrop;
use metrique::Slot;
use metrique::SlotGuard;
use thiserror::Error;

use crate::traits::ThreadSafeCloseEntry;
use crate::types::aws_smithy_http_server::request::FromParts;
use crate::types::aws_smithy_http_server::response::IntoResponse;
use crate::types::empty_response_body;
use crate::types::HttpRequestParts;
use crate::types::HttpResponse;
use crate::types::HttpStatusCode;

/// Type for metrics that can be extracted in operation handlers.
///
/// # Example
///
/// ```rust, ignore
/// pub async fn get_pokemon_species(
///     input: input::GetPokemonSpeciesInput,
///     state: Extension<Arc<State>>,
///     mut metrics: Metrics<PokemonOperationMetrics>,
/// ) -> Result<output::GetPokemonSpeciesOutput, error::GetPokemonSpeciesError> {
///     metrics.get_pokemon_species_metrics = Some("hello world".to_string());
/// }
/// ```
///
/// # Missing `MetricsLayer`
///
/// Extraction needs the request to have passed through a [`MetricsLayer`](crate::MetricsLayer)
/// built from a `#[smithy_metrics]` struct with a matching `#[smithy_metrics(operation)]` field.
/// If it didn't, this is treated as a misconfiguration:
///
/// - With debug assertions enabled, extraction panics, so tests that call
///   the operation surface the problem.
/// - Otherwise, extraction fails with [`MetricsError::UnknownExtension`], which responds with a
///   500 Internal Server Error.
///
/// Use [`Metrics::test`] to call handlers directly in unit tests.
///
pub struct Metrics<T>
where
    T: ThreadSafeCloseEntry,
{
    /// The inner slot guard that handlers can modify
    guard: SlotGuard<T>,
    /// Keeps the outer guard alive so the inner slot has a parent to send values back to
    _parent: Arc<Mutex<SlotGuard<Slot<T>>>>,
}

impl<T> Metrics<T>
where
    T: ThreadSafeCloseEntry,
{
    /// Create a test instance that discards metrics
    ///
    /// Use this in tests when you need to provide metrics to handlers
    /// but don't care about capturing the output.
    pub fn test() -> Self
    where
        T: Default,
    {
        let inner_slot = Slot::new(T::default());
        let mut outer_slot = Slot::new(inner_slot);
        let outer_guard = outer_slot.open(OnParentDrop::Discard).unwrap();
        let parent = Arc::new(Mutex::new(outer_guard));
        let guard = parent
            .lock()
            .unwrap()
            .deref_mut()
            .open(OnParentDrop::Discard)
            .unwrap();

        Self {
            guard,
            _parent: parent,
        }
    }
}

impl<T> Deref for Metrics<T>
where
    T: ThreadSafeCloseEntry,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard
    }
}

impl<T> DerefMut for Metrics<T>
where
    T: ThreadSafeCloseEntry,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.guard
    }
}

impl<Protocol, T> FromParts<Protocol> for Metrics<T>
where
    T: ThreadSafeCloseEntry,
    T::Closed: Send,
{
    type Rejection = MetricsError;

    fn from_parts(parts: &mut HttpRequestParts) -> Result<Self, Self::Rejection> {
        // Get a reference to the MetricsInExtensions without removing it from the request.
        // This keeps the outer guard alive in the extensions.
        let Some(metrics_slot) = parts.extensions.get::<MetricsExtension<T>>() else {
            // No metrics layer populated this request. Fail loudly in debug builds so tests catch
            // the misconfiguration with a clear message.
            debug_assert!(
                false,
                "`Metrics<{}>` was extracted in an operation handler, but no `MetricsLayer` added \
                these metrics to the request. Add a `MetricsLayer` built from a `#[smithy_metrics]` \
                struct with a `#[smithy_metrics(operation)]` field of this type. In release builds, \
                the request fails with a 500 instead.",
                std::any::type_name::<T>()
            );
            return Err(MetricsError::UnknownExtension);
        };

        // Clone the Arc to keep a reference to the outer guard.
        // This ensures the outer guard stays alive until this ExtensionMetrics is dropped.
        let arc_clone = Arc::clone(&metrics_slot.inner);

        // Lock the mutex and open the inner slot through deref_mut.
        // The outer SlotGuard<Slot<T>> derefs to Slot<T>, so we can call .open() on it.
        let metrics_guard = arc_clone
            .lock()
            .expect("we only hold the lock while removing, never panics")
            .open(OnParentDrop::Discard)
            .ok_or(MetricsError::MetricAlreadyOpened)?;

        Ok(Metrics {
            guard: metrics_guard,
            _parent: arc_clone, // Keep outer guard alive
        })
    }
}

/// Wrapper for storing metrics in request extensions. This type should not be used directly
/// and is only public for usage in aws-smithy-http-server-metrics-macro. See [`Metrics`].
///
/// Wraps a `SlotGuard<Slot<T>>` (the outer guard) in `Arc<Mutex<...>>`
/// to allow it to be cloned and shared across the request lifecycle while remaining
/// in the request extensions. The double slot allows for a better public API via [`Metrics`].
#[doc(hidden)]
#[derive(Clone)]
pub struct MetricsExtension<T>
where
    T: ThreadSafeCloseEntry,
{
    inner: Arc<Mutex<SlotGuard<Slot<T>>>>,
}
impl<T> MetricsExtension<T>
where
    T: ThreadSafeCloseEntry,
{
    /// Intended to be used by `aws_smithy_http_metrics_macro` only
    ///
    /// Takes a [`SlotGuard<T>`] to construct an instance of this wrapper type
    #[doc(hidden)]
    pub fn __macro_new(slotguard: SlotGuard<Slot<T>>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(slotguard)),
        }
    }
}

/// A type of error which can be returned when attempting to set metrics.
#[derive(Error, Debug)]
pub enum MetricsError {
    /// Returned in release builds when no `MetricsLayer` added the metrics to the request.
    /// See [Missing `MetricsLayer`](Metrics#missing-metricslayer).
    #[error("Unknown extension")]
    UnknownExtension,
    #[error("Slot has already been opened. Perhaps the extensions were cloned and the slot was opened multiple times.")]
    MetricAlreadyOpened,
}

impl<Protocol> IntoResponse<Protocol> for MetricsError {
    fn into_response(self) -> HttpResponse {
        let mut response = HttpResponse::new(empty_response_body());
        *response.status_mut() = HttpStatusCode::INTERNAL_SERVER_ERROR;
        response
    }
}

#[cfg(test)]
mod tests {
    use metrique::unit_of_work::metrics;

    use super::*;
    use crate::types::GenericHttpRequest;

    #[metrics]
    #[derive(Default, Clone)]
    struct TestOperationMetrics {
        value: Option<u64>,
    }

    fn extract(
        parts: &mut HttpRequestParts,
    ) -> Result<Metrics<TestOperationMetrics>, MetricsError> {
        <Metrics<TestOperationMetrics> as FromParts<()>>::from_parts(parts)
    }

    fn empty_parts() -> HttpRequestParts {
        GenericHttpRequest::new(()).into_parts().0
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "no `MetricsLayer` added these metrics")]
    fn missing_extension_panics_in_debug_builds() {
        let _ = extract(&mut empty_parts());
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn missing_extension_is_an_error_in_release_builds() {
        assert!(matches!(
            extract(&mut empty_parts()),
            Err(MetricsError::UnknownExtension)
        ));
    }
}
