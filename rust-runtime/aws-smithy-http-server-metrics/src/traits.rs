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
pub trait ThreadSafeEntrySink<Entry>:
    EntrySink<RootEntry<Entry::Closed>> + Send + Sync + 'static
where
    Entry: CloseEntry + Send + Sync + 'static,
{
}
impl<T, Entry> ThreadSafeEntrySink<Entry> for T
where
    Entry: CloseEntry + Send + Sync + 'static,
    T: EntrySink<RootEntry<Entry::Closed>> + Send + Sync + 'static,
{
}

/// Helper trait for functions that initialize a metrics entry for each request
///
/// # Example
///
/// ```rust,ignore
/// .init_metrics(|req| {
///     MyMetrics::default().append_on_drop(MetricsSink::default())
/// })
/// ```
pub trait InitMetrics<Entry, Sink>:
    Fn(&mut Request<ReqBody>) -> AppendAndCloseOnDrop<Entry, Sink> + Clone + Send + Sync + 'static
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
{
}
impl<T, Entry, Sink> InitMetrics<Entry, Sink> for T
where
    Entry: ThreadSafeCloseEntry,
    Sink: ThreadSafeEntrySink<Entry>,
    T: Fn(&mut Request<ReqBody>) -> AppendAndCloseOnDrop<Entry, Sink>
        + Clone
        + Send
        + Sync
        + 'static,
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
pub trait ResponseMetrics<Entry>:
    Fn(&mut Response<ResBody>, &mut Entry) + Clone + Send + Sync + 'static
where
    Entry: ThreadSafeCloseEntry,
{
}
impl<T, Entry> ResponseMetrics<Entry> for T
where
    Entry: ThreadSafeCloseEntry,
    T: Fn(&mut Response<ResBody>, &mut Entry) + Clone + Send + Sync + 'static,
{
}
