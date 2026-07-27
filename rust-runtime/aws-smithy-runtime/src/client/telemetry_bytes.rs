/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Real transferred-byte counting for telemetry.
//!
//! The request and response bodies are wrapped in a counter that tallies bytes per frame as
//! they flow. `Content-Length` is not used because it is `None`/`0` for streaming bodies. The
//! counts land in the `ConfigBag` for the metrics interceptor to read at emission.

use aws_smithy_runtime_api::box_error::BoxError;
use aws_smithy_runtime_api::client::interceptors::context::{
    BeforeDeserializationInterceptorContextMut, BeforeTransmitInterceptorContextMut,
};
use aws_smithy_runtime_api::client::interceptors::Intercept;
use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
use aws_smithy_types::body::SdkBody;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use http_body_1x::{Body, Frame};
use std::mem;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

// A u64 counter shared across the request/response body wraps. `AtomicU64` is avoided because
// it is unavailable on 32-bit targets (e.g. powerpc); a `Mutex` works everywhere and the
// per-frame update rate makes contention a non-issue.
type Counter = Arc<Mutex<u64>>;

fn add(counter: &Counter, n: u64) {
    if let Ok(mut total) = counter.lock() {
        *total += n;
    }
}

fn get(counter: &Counter) -> u64 {
    counter.lock().map(|t| *t).unwrap_or_default()
}

/// Shared, thread-safe byte counters for a single operation, stored in the `ConfigBag`.
///
/// Request and response are wrapped in different phases and may be polled from different
/// tasks, so the counters are shared via `Arc`.
#[derive(Clone, Debug, Default)]
pub(crate) struct TransferredBytes {
    request: Counter,
    response: Counter,
}

impl TransferredBytes {
    /// Bytes observed flowing through the request body so far.
    pub(crate) fn request_bytes(&self) -> u64 {
        get(&self.request)
    }

    /// Bytes observed flowing through the response body so far.
    pub(crate) fn response_bytes(&self) -> u64 {
        get(&self.response)
    }
}

impl Storable for TransferredBytes {
    type Storer = StoreReplace<Self>;
}

/// Body wrapper that adds each data frame's length to `counter` as the frame passes through.
/// Contents are forwarded unchanged.
struct CountingBody<B> {
    inner: B,
    counter: Counter,
}

impl<B> Body for CountingBody<B>
where
    B: Body<Data = bytes::Bytes, Error = BoxError> + Unpin,
{
    type Data = bytes::Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    // Count only data frames; trailers carry no payload bytes.
                    add(&this.counter, data.len() as u64);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            other => other,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body_1x::SizeHint {
        self.inner.size_hint()
    }
}

/// Wraps `body` so bytes flowing through it accumulate into `counter`, preserving contents.
fn wrap(body: SdkBody, counter: Counter) -> SdkBody {
    body.map_preserve_contents(move |b| {
        SdkBody::from_body_1_x(CountingBody {
            inner: b,
            counter: counter.clone(),
        })
    })
}

/// Installs byte counters on the request and response bodies and publishes the shared
/// [`TransferredBytes`] into the `ConfigBag` for the metrics interceptor to read.
#[derive(Debug, Default)]
pub(crate) struct TelemetryBytesInterceptor;

impl Intercept for TelemetryBytesInterceptor {
    fn name(&self) -> &'static str {
        "TelemetryBytesInterceptor"
    }

    fn modify_before_transmit(
        &self,
        context: &mut BeforeTransmitInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // One counter set per operation; created here (the first body we see) and reused for
        // the response wrap so both counts share a single bag entry.
        let counters = TransferredBytes::default();
        let request = counters.request.clone();
        cfg.interceptor_state().store_put(counters);

        let body = mem::replace(context.request_mut().body_mut(), SdkBody::taken());
        *context.request_mut().body_mut() = wrap(body, request);
        Ok(())
    }

    fn modify_before_deserialization(
        &self,
        context: &mut BeforeDeserializationInterceptorContextMut<'_>,
        _runtime_components: &RuntimeComponents,
        cfg: &mut ConfigBag,
    ) -> Result<(), BoxError> {
        // Reuse the counters created in `modify_before_transmit` so request and response
        // counts live in the same entry the metrics interceptor reads.
        let response = cfg
            .load::<TransferredBytes>()
            .map(|c| c.response.clone())
            .unwrap_or_default();

        let body = mem::replace(context.response_mut().body_mut(), SdkBody::taken());
        *context.response_mut().body_mut() = wrap(body, response);
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures_util::StreamExt;
    use http_body_util::BodyExt;

    async fn drain(body: SdkBody) {
        let _ = body.collect().await.expect("body drains");
    }

    #[tokio::test]
    async fn counts_all_bytes_flowing_through_a_streaming_body() {
        // A streaming body has no Content-Length, so the wrapper must count the real bytes.
        let counter = Counter::default();
        let stream = futures_util::stream::iter(vec![
            Ok::<_, BoxError>(bytes::Bytes::from_static(b"hello ")),
            Ok(bytes::Bytes::from_static(b"world")),
        ]);
        let streaming = SdkBody::from_body_1_x(http_body_util::StreamBody::new(
            stream.map(|r| r.map(Frame::data)),
        ));
        assert_eq!(None, streaming.content_length(), "precondition: streaming");

        drain(wrap(streaming, counter.clone())).await;

        assert_eq!(11, get(&counter));
    }

    #[tokio::test]
    async fn empty_body_counts_zero() {
        let counter = Counter::default();
        drain(wrap(SdkBody::empty(), counter.clone())).await;
        assert_eq!(0, get(&counter));
    }

    #[test]
    fn transferred_bytes_defaults_to_zero() {
        let tb = TransferredBytes::default();
        assert_eq!(0, tb.request_bytes());
        assert_eq!(0, tb.response_bytes());
    }
}
