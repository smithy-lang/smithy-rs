/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use http::Request;
use http::Response;
use metrique::AppendAndCloseOnDrop;
use metrique::RootEntry;
use metrique_core::CloseEntry;
use metrique_writer::EntrySink;

use crate::types::ReqBody;
use crate::types::ResBody;

/// A thread safe [`CloseEntry`]
pub trait ThreadSafeCloseEntry: CloseEntry + Send + Sync + 'static {}
impl<T> ThreadSafeCloseEntry for T where T: CloseEntry + Send + Sync + 'static {}

/// A thread safe [`EntrySink`]
pub trait ThreadSafeEntrySink<E>: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static
where
    E: CloseEntry + Send + Sync + 'static,
{
}
impl<T, E> ThreadSafeEntrySink<E> for T
where
    E: CloseEntry + Send + Sync + 'static,
    T: EntrySink<RootEntry<E::Closed>> + Send + Sync + 'static,
{
}

/// Helper trait for functions that initialize a metrics entry for each request
///
/// # Example
///
/// ```rust,ignore
/// .init_metrics(|| {
///     MyMetrics::default().append_on_drop(MetricsSink::default())
/// })
/// ```
pub trait InitMetrics<E, S>:
    Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
{
}
impl<T, E, S> InitMetrics<E, S> for T
where
    E: ThreadSafeCloseEntry,
    S: ThreadSafeEntrySink<E>,
    T: Fn() -> AppendAndCloseOnDrop<E, S> + Clone + Send + Sync + 'static,
{
}

/// Helper trait for functions that populate metrics from incoming requests
///
/// # Example
///
/// ```rust,ignore
/// .request_metrics(|request, metrics| {
///     metrics.user_agent = request
///         .headers()
///         .get("user-agent")
///         .and_then(|v| v.to_str().ok())
///         .map(Into::into);
/// })
/// ```
pub trait RequestMetrics<E>:
    Fn(&mut Request<ReqBody>, &mut E) + Clone + Send + Sync + 'static
where
    E: ThreadSafeCloseEntry,
{
}
impl<T, E> RequestMetrics<E> for T
where
    E: ThreadSafeCloseEntry,
    T: Fn(&mut Request<ReqBody>, &mut E) + Clone + Send + Sync + 'static,
{
}

/// Helper trait for functions that populate metrics from responses
///
/// # Example
///
/// ```rust,ignore
/// .response_metrics(|response, metrics| {
///     metrics.content_length = response
///         .headers()
///         .get("content-length")
///         .and_then(|v| v.to_str().ok())
///         .and_then(|v| v.parse().ok());
/// })
/// ```
pub trait ResponseMetrics<E>:
    Fn(&mut Response<ResBody>, &mut E) + Clone + Send + Sync + 'static
where
    E: ThreadSafeCloseEntry,
{
}
impl<T, E> ResponseMetrics<E> for T
where
    E: ThreadSafeCloseEntry,
    T: Fn(&mut Response<ResBody>, &mut E) + Clone + Send + Sync + 'static,
{
}
