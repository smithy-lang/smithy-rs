/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http_server::body::{
    wrap_streaming_body, wrap_streaming_body_with_chunk_budget, DEFAULT_STREAMING_CHUNK_BUDGET,
};
use bytes::Bytes;
use http_body::{Body, Frame};
use http_body_util::{Full, StreamBody};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

#[derive(Default)]
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn streaming_budget_yields_and_forwards_data_empty_frames_trailers_and_errors() {
    for budget in [None, NonZeroUsize::new(1), NonZeroUsize::new(7)] {
        let polls = Arc::new(AtomicUsize::new(0));
        let observed = polls.clone();
        let frames = futures_util::stream::poll_fn(move |_| {
            let index = observed.fetch_add(1, Ordering::SeqCst);
            Poll::Ready(match index {
                0 => Some(Ok(Frame::data(Bytes::from_static(b"payload")))),
                1..=64 => Some(Ok(Frame::data(Bytes::new()))),
                65 => {
                    let mut trailers = http::HeaderMap::new();
                    trailers.insert("x-trailer", "preserved".parse().unwrap());
                    Some(Ok(Frame::trailers(trailers)))
                }
                66 => Some(Err(std::io::Error::other("body failure"))),
                _ => None,
            })
        });
        let inner = StreamBody::new(frames);
        let body = match budget {
            None => wrap_streaming_body(inner),
            Some(budget) => wrap_streaming_body_with_chunk_budget(inner, budget),
        };
        let mut body = Box::pin(body);
        let wakes = Arc::new(WakeCount::default());
        let waker = Waker::from(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        let expected_budget = budget.unwrap_or(DEFAULT_STREAMING_CHUNK_BUDGET).get();
        let mut yields = 0;
        let mut forwarded = 0;
        loop {
            match body.as_mut().poll_frame(&mut cx) {
                Poll::Pending => {
                    yields += 1;
                    assert_eq!(polls.load(Ordering::SeqCst), yields * expected_budget);
                    assert_eq!(wakes.0.load(Ordering::SeqCst), yields);
                }
                Poll::Ready(Some(frame)) => {
                    match forwarded {
                        0 => assert_eq!(frame.unwrap().into_data().unwrap(), "payload"),
                        1..=64 => assert!(frame.unwrap().into_data().unwrap().is_empty()),
                        65 => assert_eq!(frame.unwrap().into_trailers().unwrap()["x-trailer"], "preserved"),
                        66 => assert_eq!(frame.unwrap_err().to_string(), "body failure"),
                        _ => panic!("unexpected frame"),
                    }
                    forwarded += 1;
                }
                Poll::Ready(None) => break,
            }
        }
        assert_eq!(forwarded, 67);
        assert_eq!(polls.load(Ordering::SeqCst), 68);
        assert_eq!(yields, 67 / expected_budget);
    }
}

#[test]
fn underlying_pending_resets_streaming_budget_and_trailers_consume_it() {
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let frames = futures_util::stream::poll_fn(move |cx| {
        let index = observed.fetch_add(1, Ordering::SeqCst);
        match index {
            0 | 2 | 4 => Poll::Ready(Some(Ok::<_, std::convert::Infallible>(Frame::data(Bytes::new())))),
            1 => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            3 => Poll::Ready(Some(Ok(Frame::trailers(http::HeaderMap::new())))),
            _ => Poll::Ready(None),
        }
    });
    let body = wrap_streaming_body_with_chunk_budget(StreamBody::new(frames), NonZeroUsize::new(3).unwrap());
    let mut body = Box::pin(body);
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert!(matches!(body.as_mut().poll_frame(&mut cx), Poll::Ready(Some(Ok(_)))));
    assert!(body.as_mut().poll_frame(&mut cx).is_pending());
    assert_eq!(polls.load(Ordering::SeqCst), 2);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    for _ in 0..3 {
        assert!(matches!(body.as_mut().poll_frame(&mut cx), Poll::Ready(Some(Ok(_)))));
    }
    assert_eq!(polls.load(Ordering::SeqCst), 5);
    assert!(body.as_mut().poll_frame(&mut cx).is_pending());
    assert_eq!(
        polls.load(Ordering::SeqCst),
        5,
        "forced yield must not poll the inner body"
    );
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
    assert!(matches!(body.as_mut().poll_frame(&mut cx), Poll::Ready(None)));
}

#[tokio::test]
async fn streaming_wrapper_preserves_size_hints_and_end_of_stream() {
    use http_body_util::BodyExt;

    let mut body = wrap_streaming_body(Full::new(Bytes::from_static(b"payload")));
    assert_eq!(body.size_hint().exact(), Some(7));
    assert!(!body.is_end_stream());
    assert_eq!(body.frame().await.unwrap().unwrap().into_data().unwrap(), "payload");
    assert_eq!(body.size_hint().exact(), Some(0));
    assert!(body.is_end_stream());
    assert!(body.frame().await.is_none());
}

#[test]
fn event_stream_receiver_yields_while_reading_an_incomplete_message() {
    use aws_smithy_eventstream::frame::{write_message_to, UnmarshallMessage, UnmarshalledMessage};
    use aws_smithy_http::event_stream::Receiver;
    use aws_smithy_types::body::SdkBody;
    use aws_smithy_types::event_stream::Message;
    use std::future::Future;

    #[derive(Debug)]
    struct Unmarshaller;
    impl UnmarshallMessage for Unmarshaller {
        type Output = Bytes;
        type Error = std::convert::Infallible;
        fn unmarshall(
            &self,
            message: &Message,
        ) -> Result<UnmarshalledMessage<Bytes, Self::Error>, aws_smithy_eventstream::error::Error> {
            Ok(UnmarshalledMessage::Event(message.payload().clone()))
        }
    }

    let payload = Bytes::from_static(b"event payload");
    let mut encoded = Vec::new();
    write_message_to(&Message::new(payload.clone()), &mut encoded).unwrap();
    let encoded = Bytes::from(encoded);
    let polls = Arc::new(AtomicUsize::new(0));
    let observed = polls.clone();
    let frames = futures_util::stream::poll_fn(move |_| {
        let index = observed.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Some(Ok::<_, std::convert::Infallible>(Frame::data(match index {
            0 => encoded.slice(..13),
            1..=64 => Bytes::new(),
            65 => encoded.slice(13..),
            _ => panic!("receiver should stop reading after the message"),
        }))))
    });
    let body = SdkBody::from_body_1_x(wrap_streaming_body(StreamBody::new(frames)));
    let mut receiver = Receiver::new(Unmarshaller, body);
    let mut recv = Box::pin(receiver.recv());
    let wakes = Arc::new(WakeCount::default());
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    for expected_polls in [32, 64] {
        assert!(recv.as_mut().poll(&mut cx).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), expected_polls);
    }
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
    let Poll::Ready(Ok(Some(event))) = recv.as_mut().poll(&mut cx) else {
        panic!("receiver should decode the completed message after yielding");
    };
    assert_eq!(event, payload);
    assert_eq!(polls.load(Ordering::SeqCst), 66);
}
