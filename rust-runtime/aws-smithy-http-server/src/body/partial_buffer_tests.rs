/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::{Body, RequestBody};
use bytes::Bytes;
use http_body::{Body as _, Frame, SizeHint};
use http_body_util::{BodyExt, Full, StreamBody};
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

fn transport_body<B>(body: B) -> RequestBody
where
    B: http_body::Body<Data = Bytes, Error = Infallible> + Send + Sync + 'static,
{
    RequestBody::new(body)
}

#[tokio::test]
async fn partial_buffer_replays_prefix_then_tail_data_and_trailers() {
    let mut trailers = http::HeaderMap::new();
    trailers.insert("checksum", http::HeaderValue::from_static("abc"));
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let mut frames = [
        Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"json:"))),
        Ok(Frame::data(Bytes::from_static(b"payload"))),
        Ok(Frame::trailers(trailers.clone())),
    ]
    .into_iter();
    let transport = StreamBody::new(futures_util::stream::poll_fn(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(frames.next())
    }));
    // Non-Hyper sources exercise the boxed path in the current transport representation.
    let mut tail = transport_body(transport);
    let prefix = tail.frame().await.unwrap().unwrap().into_data().unwrap();
    assert_eq!(polls.load(Ordering::SeqCst), 1);
    let mut body = RequestBody::prefixed(prefix, None, tail);
    assert!(body.buffered_content().is_none());
    assert!(!body.is_end_stream());
    assert_eq!(polls.load(Ordering::SeqCst), 1, "construction must not read the tail");
    assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "json:");
    assert_eq!(
        polls.load(Ordering::SeqCst),
        1,
        "replaying the prefix must not read the tail"
    );
    assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "payload");
    assert_eq!(body.frame().await.unwrap().unwrap().into_trailers().unwrap(), trailers);
    assert!(body.frame().await.is_none());
    assert!(body.is_end_stream());
    assert_eq!(body.size_hint().exact(), Some(0));
    assert!(body.frame().await.is_none());
}

#[test]
fn partial_buffer_forwards_pending_and_tail_errors_after_the_prefix() {
    let mut polls = 0;
    let stream = futures_util::stream::poll_fn(move |cx| {
        polls += 1;
        match polls {
            1 => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            2 => Poll::Ready(Some(Err::<Frame<Bytes>, _>(std::io::Error::other("tail failed")))),
            _ => Poll::Ready(None),
        }
    });
    // Body's default transport type erases this stream into the boxed path.
    let tail = Body::new(StreamBody::new(stream));
    let mut body = Body::prefixed(Bytes::from_static(b"prefix"), None, tail);
    let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
    let mut body = std::pin::Pin::new(&mut body);
    match body.as_mut().poll_frame(&mut cx) {
        Poll::Ready(Some(Ok(frame))) => assert_eq!(frame.into_data().unwrap(), "prefix"),
        other => panic!("expected prefix frame: {other:?}"),
    }
    assert!(body.as_mut().poll_frame(&mut cx).is_pending());
    assert!(!body.is_end_stream());
    match body.as_mut().poll_frame(&mut cx) {
        Poll::Ready(Some(Err(error))) => assert!(error.to_string().contains("tail failed")),
        other => panic!("expected tail error: {other:?}"),
    }
    assert!(matches!(body.as_mut().poll_frame(&mut cx), Poll::Ready(None)));
    assert!(body.is_end_stream());
}

#[tokio::test]
async fn partial_buffer_preserves_already_read_trailers() {
    let mut trailers = http::HeaderMap::new();
    trailers.insert("checksum", http::HeaderValue::from_static("abc"));
    let frames = [
        Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"payload"))),
        Ok(Frame::trailers(trailers.clone())),
    ];
    let mut tail = Body::new(StreamBody::new(futures_util::stream::iter(frames)));
    let prefix = tail.frame().await.unwrap().unwrap().into_data().unwrap();
    let captured = tail.frame().await.unwrap().unwrap().into_trailers().unwrap();
    // The reader has seen trailers but has not yet polled end-of-stream.
    let body = Body::prefixed(prefix, Some(captured), tail);
    let collected = body.collect().await.unwrap();
    assert_eq!(collected.trailers(), Some(&trailers));
    assert_eq!(collected.to_bytes(), "payload");
}

#[tokio::test]
async fn partial_buffer_size_hint_includes_only_unread_data() {
    let tail = Body::new(Full::new(Bytes::from_static(b"tail")));
    let mut body = Body::prefixed(Bytes::from_static(b"pre"), None, tail);
    assert_eq!(body.size_hint().exact(), Some(7));
    assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "pre");
    assert_eq!(body.size_hint().exact(), Some(4));
    assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "tail");
    assert_eq!(body.size_hint().exact(), Some(0));
    assert!(body.is_end_stream());
}

#[test]
fn partial_buffer_size_hint_handles_overflow_and_unknown_upper_bounds() {
    struct HintedBody(SizeHint);
    impl http_body::Body for HintedBody {
        type Data = Bytes;
        type Error = Infallible;

        fn poll_frame(
            self: std::pin::Pin<&mut Self>,
            _: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
            panic!("checking size hints must not read the body");
        }

        fn size_hint(&self) -> SizeHint {
            self.0.clone()
        }
    }
    for (hint, lower, upper) in [
        (SizeHint::with_exact(u64::MAX), u64::MAX, None),
        (SizeHint::new(), 3, None),
    ] {
        let tail = Body::new(HintedBody(hint));
        let body = Body::prefixed(Bytes::from_static(b"pre"), None, tail);
        assert_eq!(body.size_hint().lower(), lower);
        assert_eq!(body.size_hint().upper(), upper);
    }
}

#[tokio::test]
async fn partial_buffer_empty_prefix_keeps_the_complete_body_fast_path() {
    let bytes = Bytes::from_static(b"payload");
    let tail = Body::from_bytes(bytes.clone());
    let body = Body::prefixed(Bytes::new(), None, tail);
    assert_eq!(body.buffered_content().unwrap().as_ptr(), bytes.as_ptr());
    assert_eq!(body.collect().await.unwrap().to_bytes(), bytes);
    let mut body = Body::prefixed(Bytes::new(), None, Body::empty());
    assert!(body.is_end_stream());
    assert!(body.frame().await.is_none());
}

#[tokio::test]
async fn partial_buffer_complete_collection_reads_the_tail_and_enforces_the_total_limit() {
    let body = || {
        Body::prefixed(
            Bytes::from_static(b"pre"),
            None,
            Body::from_bytes(Bytes::from_static(b"tail")),
        )
    };
    let mut config = crate::schema::RequestBodyCollectionConfig::default();
    assert_eq!(
        crate::schema::collect_request_body(body(), &config).await.unwrap(),
        "pretail"
    );
    config.max_bytes = std::num::NonZeroUsize::new(6);
    assert!(matches!(
        crate::schema::collect_request_body(body(), &config).await,
        Err(crate::schema::RequestBodyCollectionError::BodyLimitExceeded(_))
    ));
}
