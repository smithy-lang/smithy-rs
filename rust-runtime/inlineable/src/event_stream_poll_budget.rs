/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Cooperatively yields while reading event-stream HTTP request bodies.
//!
//! After `budget` consecutive ready HTTP frames, the next poll returns `Pending`
//! and schedules another poll through the task's waker without reading the inner
//! body. Empty data frames, trailers, and errors count. An underlying `Pending`
//! resets the budget. A budget of zero disables forced yielding.
//!
//! This bounds work between opportunities for the executor to schedule other
//! tasks; it does not limit bytes or events, or preempt an individual body poll.

use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) fn wrap_with_poll_budget(body: SdkBody, budget: usize) -> SdkBody {
    if budget == 0 {
        return body;
    }
    SdkBody::from_body_1_x(PollBudgetBody::new(body, budget))
}

pin_project_lite::pin_project! {
    struct PollBudgetBody<B> {
        #[pin]
        inner: B,
        budget: usize,
        consecutive_ready: usize,
    }
}

impl<B> PollBudgetBody<B> {
    fn new(inner: B, budget: usize) -> Self {
        Self {
            inner,
            budget,
            consecutive_ready: 0,
        }
    }
}

impl<B: http_body_1x::Body<Data = Bytes>> http_body_1x::Body for PollBudgetBody<B> {
    type Data = Bytes;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
        let this = self.project();
        if *this.budget != 0 && *this.consecutive_ready >= *this.budget {
            *this.consecutive_ready = 0;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        let result = this.inner.poll_frame(cx);
        if *this.budget != 0 {
            match &result {
                Poll::Ready(Some(_)) => *this.consecutive_ready += 1,
                Poll::Pending | Poll::Ready(None) => *this.consecutive_ready = 0,
            }
        }
        result
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> http_body_1x::SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::{wrap_with_poll_budget, PollBudgetBody};
    use aws_smithy_types::body::SdkBody;
    use bytes::Bytes;
    use http_body_1x::{Body, Frame};
    use std::collections::VecDeque;
    use std::io::Error as IOError;
    use std::pin::Pin;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::task::{Context, Poll, Wake, Waker};

    enum Step {
        Frame(Frame<Bytes>),
        Error,
        Pending,
    }

    struct TestBody {
        steps: VecDeque<Step>,
        polls: Arc<AtomicUsize>,
    }

    impl Body for TestBody {
        type Data = Bytes;
        type Error = IOError;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, IOError>>> {
            self.polls.fetch_add(1, Ordering::SeqCst);
            match self.steps.pop_front() {
                Some(Step::Frame(frame)) => Poll::Ready(Some(Ok(frame))),
                Some(Step::Error) => Poll::Ready(Some(Err(IOError::other("test body error")))),
                Some(Step::Pending) => Poll::Pending,
                None => Poll::Ready(None),
            }
        }
    }

    #[derive(Default)]
    struct TestWake(AtomicUsize);

    impl Wake for TestWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn body(steps: Vec<Step>, budget: usize) -> (PollBudgetBody<TestBody>, Arc<AtomicUsize>) {
        let polls = Arc::new(AtomicUsize::new(0));
        (
            PollBudgetBody::new(
                TestBody {
                    steps: steps.into(),
                    polls: polls.clone(),
                },
                budget,
            ),
            polls,
        )
    }

    fn data(bytes: &'static [u8]) -> Step {
        Step::Frame(Frame::data(Bytes::from_static(bytes)))
    }

    fn poll(
        body: &mut PollBudgetBody<TestBody>,
        wake: &Arc<TestWake>,
    ) -> Poll<Option<Result<Frame<Bytes>, IOError>>> {
        let waker = Waker::from(wake.clone());
        Pin::new(body).poll_frame(&mut Context::from_waker(&waker))
    }

    fn expect_data(body: &mut PollBudgetBody<TestBody>, wake: &Arc<TestWake>, bytes: &[u8]) {
        match poll(body, wake) {
            Poll::Ready(Some(Ok(frame))) => assert_eq!(frame.into_data().unwrap(), bytes),
            other => panic!("expected data, got {other:?}"),
        }
    }

    #[test]
    fn budget_yields_and_wakes_without_consuming_the_next_frame() {
        let (mut body, polls) = body(vec![data(b"a"), data(b"b"), data(b"c")], 2);
        let wake = Arc::new(TestWake::default());
        expect_data(&mut body, &wake, b"a");
        expect_data(&mut body, &wake, b"b");
        assert!(poll(&mut body, &wake).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), 2);
        assert_eq!(wake.0.load(Ordering::SeqCst), 1);
        expect_data(&mut body, &wake, b"c");
        assert!(matches!(poll(&mut body, &wake), Poll::Ready(None)));
    }

    #[test]
    fn underlying_pending_resets_the_budget() {
        let (mut body, _) = body(
            vec![
                data(b"a"),
                Step::Pending,
                data(b"b"),
                data(b"c"),
                data(b"d"),
            ],
            2,
        );
        let wake = Arc::new(TestWake::default());
        expect_data(&mut body, &wake, b"a");
        assert!(poll(&mut body, &wake).is_pending());
        assert_eq!(wake.0.load(Ordering::SeqCst), 0);
        expect_data(&mut body, &wake, b"b");
        expect_data(&mut body, &wake, b"c");
        assert!(poll(&mut body, &wake).is_pending());
        expect_data(&mut body, &wake, b"d");
    }

    #[test]
    fn empty_data_trailers_and_errors_count_toward_the_budget() {
        let (mut body, polls) = body(
            vec![
                data(b""),
                Step::Frame(Frame::trailers(Default::default())),
                Step::Error,
                data(b"after"),
            ],
            1,
        );
        let wake = Arc::new(TestWake::default());
        expect_data(&mut body, &wake, b"");
        assert!(poll(&mut body, &wake).is_pending());
        assert!(
            matches!(poll(&mut body, &wake), Poll::Ready(Some(Ok(frame))) if frame.trailers_ref().is_some())
        );
        assert!(poll(&mut body, &wake).is_pending());
        assert!(matches!(poll(&mut body, &wake), Poll::Ready(Some(Err(_)))));
        assert!(poll(&mut body, &wake).is_pending());
        expect_data(&mut body, &wake, b"after");
        assert_eq!(polls.load(Ordering::SeqCst), 4);
        assert_eq!(wake.0.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn zero_budget_does_not_force_yields() {
        let (mut body, _) = body((0..100).map(|_| data(b"a")).collect(), 0);
        let wake = Arc::new(TestWake::default());
        for _ in 0..100 {
            expect_data(&mut body, &wake, b"a");
        }
        assert!(matches!(poll(&mut body, &wake), Poll::Ready(None)));
        assert_eq!(wake.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn sdk_body_completes_after_multiple_forced_yields() {
        use http_body_util::BodyExt;
        let polls = Arc::new(AtomicUsize::new(0));
        let inner = SdkBody::from_body_1_x(TestBody {
            steps: (0..100).map(|_| data(b"a")).collect(),
            polls,
        });
        let result = wrap_with_poll_budget(inner, 3).collect().await.unwrap();
        assert_eq!(result.to_bytes(), Bytes::from(vec![b'a'; 100]));
    }
}
