/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Applies a per-message completion deadline to an event stream request body.
//!
//! Once the first bytes of an event stream message frame arrive, the complete frame must
//! arrive within the configured timeout, or the body errors out and the stream terminates.
//! Time spent idle *between* messages is not subject to the deadline. This mitigates
//! slow-drip request attacks against event stream operations, where a client trickles a
//! message one byte at a time to pin server resources.
//!
//! Message boundaries are tracked from the frame's 4-byte big-endian `total_length` prelude
//! field, which is the first field of every event stream message. Malformed lengths are left
//! for the event stream frame decoder downstream to reject.

use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

/// Error returned when an event stream message doesn't complete within the configured deadline.
#[derive(Debug)]
pub(crate) struct MessageTimeoutError {
    timeout: Duration,
}

impl fmt::Display for MessageTimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "an event stream message started arriving, but didn't complete within the configured timeout ({:?})",
            self.timeout
        )
    }
}

impl StdError for MessageTimeoutError {}

/// Wraps `body` so that each event stream message frame must fully arrive within `timeout`
/// of its first bytes arriving.
pub(crate) fn wrap_with_message_timeout(body: SdkBody, timeout: Duration) -> SdkBody {
    SdkBody::from_body_1_x(MessageTimeoutBody {
        inner: body,
        timeout,
        frame_remaining: 0,
        prelude: [0; PRELUDE_SIZE],
        prelude_len: 0,
        deadline: None,
    })
}

const PRELUDE_SIZE: usize = 4;

pin_project_lite::pin_project! {
    struct MessageTimeoutBody {
        #[pin]
        inner: SdkBody,
        timeout: Duration,
        // Bytes of the current frame still expected after the length prefix.
        frame_remaining: u64,
        // The frame's 4-byte big-endian `total_length` prefix, collected across chunks.
        prelude: [u8; PRELUDE_SIZE],
        prelude_len: usize,
        // Sleep future for the message currently being received; armed when the first
        // bytes of a new frame arrive and cleared once the frame completes.
        deadline: Option<Pin<Box<tokio::time::Sleep>>>,
    }
}

/// Byte-accounting state machine tracking event stream frame boundaries.
struct FrameTracker<'a> {
    frame_remaining: &'a mut u64,
    prelude: &'a mut [u8; PRELUDE_SIZE],
    prelude_len: &'a mut usize,
}

impl FrameTracker<'_> {
    /// Advances the frame state over `chunk`, returning whether any frame completed
    /// within this chunk.
    fn note_bytes(&mut self, mut chunk: &[u8]) -> bool {
        let mut completed_a_frame = false;
        while !chunk.is_empty() {
            if *self.frame_remaining > 0 {
                let take = (*self.frame_remaining).min(chunk.len() as u64) as usize;
                *self.frame_remaining -= take as u64;

                // Move the chunk slice forward by the number of bytes that are for this
                // frame so that we can check if a new frame has started in this chunk of
                // data.
                chunk = &chunk[take..];
                if *self.frame_remaining == 0 {
                    completed_a_frame = true;
                }
            } else {
                debug_assert!(
                    *self.prelude_len < PRELUDE_SIZE,
                    "the length prefix buffer always has room at a frame boundary; it is reset to 0 once full"
                );
                let take = (PRELUDE_SIZE - *self.prelude_len).min(chunk.len());
                // Accumulate the 4-byte length prefix, which may itself be split across chunks
                self.prelude[*self.prelude_len..*self.prelude_len + take]
                    .copy_from_slice(&chunk[..take]);
                *self.prelude_len += take;
                // Advance our view past the bytes just accounted for; the rest of the chunk
                // is processed on the next loop iteration
                chunk = &chunk[take..];
                if *self.prelude_len == PRELUDE_SIZE {
                    // `total_length` includes the length prefix itself. A length smaller
                    // than the prefix is malformed; the frame decoder will reject it, so
                    // just avoid underflowing here.
                    let total_length = u64::from(u32::from_be_bytes(*self.prelude));
                    *self.frame_remaining = total_length.saturating_sub(PRELUDE_SIZE as u64);
                    *self.prelude_len = 0;
                    if *self.frame_remaining == 0 {
                        completed_a_frame = true;
                    }
                }
            }
        }
        completed_a_frame
    }

    /// Returns true if a frame has started arriving but hasn't completed.
    fn is_mid_frame(&self) -> bool {
        *self.frame_remaining > 0 || *self.prelude_len > 0
    }
}

impl http_body_1x::Body for MessageTimeoutBody {
    type Data = Bytes;
    type Error = aws_smithy_types::body::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Bytes>, Self::Error>>> {
        let this = self.project();
        match this.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    let mut tracker = FrameTracker {
                        frame_remaining: this.frame_remaining,
                        prelude: this.prelude,
                        prelude_len: this.prelude_len,
                    };
                    if tracker.note_bytes(data) {
                        // A frame completed in this chunk: its deadline no longer applies.
                        // If the chunk also contains the start of the next frame, a fresh
                        // deadline is armed below.
                        *this.deadline = None;
                    }
                    if tracker.is_mid_frame() && this.deadline.is_none() {
                        *this.deadline = Some(Box::pin(tokio::time::sleep(*this.timeout)));
                    } else if !tracker.is_mid_frame() {
                        *this.deadline = None;
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(other) => Poll::Ready(other),
            Poll::Pending => {
                if let Some(deadline) = this.deadline.as_mut() {
                    if deadline.as_mut().poll(cx).is_ready() {
                        return Poll::Ready(Some(Err(Box::new(MessageTimeoutError {
                            timeout: *this.timeout,
                        }))));
                    }
                }
                Poll::Pending
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        http_body_1x::Body::is_end_stream(&self.inner)
    }

    fn size_hint(&self) -> http_body_1x::SizeHint {
        http_body_1x::Body::size_hint(&self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::wrap_with_message_timeout;
    use aws_smithy_types::body::SdkBody;
    use bytes::Bytes;
    use http_body_1x::{Body, Frame};
    use std::collections::VecDeque;
    use std::future::Future;
    use std::io::Error as IOError;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;

    /// Encodes a minimal event stream frame with the given payload size: the 4-byte
    /// total length prefix followed by arbitrary bytes (the tracker only reads lengths).
    fn encode_frame(payload_size: usize) -> Bytes {
        let total_length = (4 + payload_size) as u32;
        let mut frame = total_length.to_be_bytes().to_vec();
        frame.extend(std::iter::repeat(0u8).take(payload_size));
        frame.into()
    }

    enum Step {
        Data(Bytes),
        DelayThenData(Duration, Bytes),
        Hang,
    }

    /// A test body that yields data chunks with optional delays, or hangs forever.
    struct StepBody {
        steps: VecDeque<Step>,
        sleep: Option<Pin<Box<tokio::time::Sleep>>>,
    }

    impl StepBody {
        fn new(steps: Vec<Step>) -> Self {
            Self {
                steps: steps.into(),
                sleep: None,
            }
        }
    }

    impl Body for StepBody {
        type Data = Bytes;
        type Error = IOError;

        fn poll_frame(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Frame<Bytes>, IOError>>> {
            loop {
                if let Some(sleep) = self.sleep.as_mut() {
                    match sleep.as_mut().poll(cx) {
                        Poll::Ready(()) => self.sleep = None,
                        Poll::Pending => return Poll::Pending,
                    }
                }
                match self.steps.pop_front() {
                    Some(Step::Data(data)) => return Poll::Ready(Some(Ok(Frame::data(data)))),
                    Some(Step::DelayThenData(delay, data)) => {
                        self.sleep = Some(Box::pin(tokio::time::sleep(delay)));
                        self.steps.push_front(Step::Data(data));
                    }
                    // Hang forever; only the wrapper's message deadline can wake the task
                    Some(Step::Hang) => {
                        self.steps.push_front(Step::Hang);
                        return Poll::Pending;
                    }
                    None => return Poll::Ready(None),
                }
            }
        }
    }

    fn wrapped_body(steps: Vec<Step>, timeout: Duration) -> SdkBody {
        wrap_with_message_timeout(SdkBody::from_body_1_x(StepBody::new(steps)), timeout)
    }

    async fn collect_body(mut body: SdkBody) -> Result<Vec<Bytes>, String> {
        use http_body_util::BodyExt;
        let mut chunks = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|err| err.to_string())?;
            if let Ok(data) = frame.into_data() {
                chunks.push(data);
            }
        }
        Ok(chunks)
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_does_not_apply_between_messages() {
        let body = wrapped_body(
            vec![
                Step::Data(encode_frame(20)),
                // A long gap *between* messages must not trip the per-message timeout
                Step::DelayThenData(Duration::from_secs(3600), encode_frame(20)),
            ],
            Duration::from_millis(100),
        );
        let chunks = collect_body(body).await.expect("no timeout expected");
        assert_eq!(2, chunks.len());
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fires_when_message_stalls_mid_frame() {
        let frame = encode_frame(20);
        let body = wrapped_body(
            vec![Step::Data(frame.slice(0..10)), Step::Hang],
            Duration::from_millis(100),
        );
        let err = collect_body(body).await.expect_err("expected a timeout");
        assert!(
            err.contains("didn't complete within the configured timeout"),
            "{err}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fires_when_stalled_mid_length_prefix() {
        let frame = encode_frame(20);
        let body = wrapped_body(
            // Stall with only 2 of the 4 length prefix bytes received
            vec![Step::Data(frame.slice(0..2)), Step::Hang],
            Duration::from_millis(100),
        );
        collect_body(body).await.expect_err("expected a timeout");
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fires_on_slow_drip() {
        let frame = encode_frame(20);
        let len = frame.len();
        let body = wrapped_body(
            vec![
                Step::DelayThenData(Duration::from_millis(60), frame.slice(0..8)),
                Step::DelayThenData(Duration::from_millis(60), frame.slice(8..16)),
                // Deadline armed when the first chunk arrives expires before this chunk lands
                Step::DelayThenData(Duration::from_millis(60), frame.slice(16..len)),
            ],
            Duration::from_millis(100),
        );
        collect_body(body).await.expect_err("expected a timeout");
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fires_on_one_byte_at_a_time_drip() {
        // A client dripping a frame one byte per 50ms never completes a 24-byte frame
        // within the 100ms deadline, no matter how steadily it keeps sending
        let frame = encode_frame(20);
        let steps = frame
            .iter()
            .map(|byte| {
                Step::DelayThenData(Duration::from_millis(50), Bytes::copy_from_slice(&[*byte]))
            })
            .collect();
        let body = wrapped_body(steps, Duration::from_millis(100));
        collect_body(body).await.expect_err("expected a timeout");
    }

    #[tokio::test(start_paused = true)]
    async fn one_byte_chunks_within_timeout_succeed() {
        // Single-byte chunks (including the length prefix split into 4 chunks) are fine
        // as long as the whole frame arrives within the deadline
        let frame = encode_frame(20);
        let steps = frame
            .iter()
            .map(|byte| Step::Data(Bytes::copy_from_slice(&[*byte])))
            .collect();
        let body = wrapped_body(steps, Duration::from_millis(100));
        let chunks = collect_body(body).await.expect("no timeout expected");
        assert_eq!(frame.len(), chunks.len());
    }

    #[tokio::test(start_paused = true)]
    async fn message_completes_within_timeout() {
        let frame = encode_frame(20);
        let len = frame.len();
        let body = wrapped_body(
            vec![
                Step::Data(frame.slice(0..8)),
                Step::DelayThenData(Duration::from_millis(50), frame.slice(8..len)),
            ],
            Duration::from_millis(100),
        );
        let chunks = collect_body(body).await.expect("no timeout expected");
        assert_eq!(2, chunks.len());
    }

    #[tokio::test(start_paused = true)]
    async fn deadline_resets_for_frame_starting_in_same_chunk() {
        // One chunk ends frame 1 and starts frame 2; frame 2's remainder arrives within
        // the timeout of *its* start, even though frame 1 started long before.
        let frame1 = encode_frame(20);
        let frame2 = encode_frame(20);
        let split = frame2.len() / 2;
        let mut boundary_chunk = frame1.slice(4..).to_vec();
        boundary_chunk.extend_from_slice(&frame2.slice(0..split));
        let body = wrapped_body(
            vec![
                Step::Data(frame1.slice(0..4)),
                // Frame 1 completes here at t=60ms (within its 100ms deadline), and frame
                // 2 starts; frame 2's remainder arrives 60ms later, within its own deadline.
                Step::DelayThenData(Duration::from_millis(60), boundary_chunk.into()),
                Step::DelayThenData(Duration::from_millis(60), frame2.slice(split..)),
            ],
            Duration::from_millis(100),
        );
        let chunks = collect_body(body).await.expect("no timeout expected");
        assert_eq!(3, chunks.len());
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_fires_when_frame_started_in_boundary_chunk_stalls() {
        // One chunk ends frame 1 and starts frame 2, then the client goes silent: the
        // fresh deadline armed for frame 2 must still fire.
        let frame1 = encode_frame(20);
        let frame2 = encode_frame(20);
        let mut boundary_chunk = frame1.to_vec();
        boundary_chunk.extend_from_slice(&frame2.slice(0..frame2.len() / 2));
        let body = wrapped_body(
            vec![Step::Data(boundary_chunk.into()), Step::Hang],
            Duration::from_millis(100),
        );
        let err = collect_body(body).await.expect_err("expected a timeout");
        assert!(
            err.contains("didn't complete within the configured timeout"),
            "{err}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn multiple_frames_in_one_chunk() {
        let mut combined = encode_frame(10).to_vec();
        combined.extend_from_slice(&encode_frame(30));
        let body = wrapped_body(
            vec![Step::Data(combined.into())],
            Duration::from_millis(100),
        );
        let chunks = collect_body(body).await.expect("no timeout expected");
        assert_eq!(1, chunks.len());
    }
}
