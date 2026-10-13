/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Applies initial-message, per-message completion, and idle deadlines to an event stream request body.
//!
//! Once the first bytes of an event stream message frame are read, the complete frame must
//! be read within the configured timeout, or the body reports a resumable timeout error.
//! A separate idle deadline bounds waiting for the next message's first bytes. This mitigates
//! slow-drip request attacks against event stream operations, where a client trickles a
//! message one byte at a time to pin server resources.
//!
//! RPC initial reads use a separate deadline, armed before waiting for any bytes and
//! kept until the first frame completes. The per-event deadline starts only after
//! that frame. Initial, completion, and idle deadlines reuse the same timer.
//!
//! Message boundaries are tracked from the frame's 4-byte big-endian `total_length` prelude
//! field, which is the first field of every event stream message. Malformed lengths are left
//! for the event stream frame decoder downstream to reject.
//!
//! Deadlines are checked before polling for more body frames. Time spent between polls
//! counts toward the current deadline, including idle gaps, handler backpressure, and
//! scheduling delays, even when the remaining bytes are already buffered.
//!
//! # Handling timeouts in your service handler
//!
//! The timeout surfaces from the event stream receiver's `recv()` call as an error whose
//! source chain contains [`MessageTimeoutError`]. The error is *resumable*: the stream is not
//! torn down when it fires. Your handler decides what happens next:
//!
//! - **Stop processing the request (default):** propagate the error (e.g. with `?`) or return from the handler.
//!   This is what happens if you don't special-case the error, so handlers that don't opt in
//!   keep the hard-reject behavior. The timeout does not itself close the transport connection.
//! - **Keep waiting:** call `recv()` again. This grants the in-flight message one more timeout
//!   window; if it stalls again, another error is returned. Count occurrences and give up after
//!   a budget to avoid re-opening the slow-sender vector:
//!
//! ```ignore
//! let mut strikes = 0;
//! loop {
//!     match input.stream.recv().await {
//!         Ok(Some(event)) => { /* handle event */ }
//!         Ok(None) => break,
//!         Err(err) if MessageTimeoutError::find_in_chain(&err).is_some() && strikes < 3 => {
//!             strikes += 1;
//!             continue; // grant one more timeout window
//!         }
//!         Err(err) => return Err(err.into()), // stop processing the request
//!     }
//! }
//! ```

use aws_smithy_types::body::SdkBody;
use bytes::Bytes;
use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

/// Error returned when an event stream initial, completion, or idle deadline expires.
///
/// This error is resumable: receiving it does not terminate the stream. Calling `recv()` on
/// the event stream receiver again resumes reading and grants the in-flight message a fresh
/// timeout window. Propagating the error out of the handler ends request processing;
/// the timeout does not itself close the transport connection.
#[derive(Debug)]
pub enum MessageTimeoutError {
    /// The RPC initial frame did not arrive completely before its deadline.
    InitialMessage { timeout: Duration },
    /// An event frame started arriving but did not complete before its deadline.
    MessageCompletion { timeout: Duration },
    /// The next event's first bytes did not arrive before the idle deadline.
    Idle { timeout: Duration },
}

impl MessageTimeoutError {
    /// The configured initial-message, completion, or idle timeout that expired.
    pub fn timeout(&self) -> Duration {
        match self {
            Self::InitialMessage { timeout }
            | Self::MessageCompletion { timeout }
            | Self::Idle { timeout } => *timeout,
        }
    }

    /// Whether this deadline covered the RPC initial read, including its first-byte wait.
    /// Generated servers reject these errors before invoking the operation handler.
    pub fn is_initial_message(&self) -> bool {
        matches!(self, Self::InitialMessage { .. })
    }

    /// Whether the deadline expired while waiting for the next event's first bytes.
    pub fn is_idle(&self) -> bool {
        matches!(self, Self::Idle { .. })
    }

    /// Searches `err` and its source chain for a `MessageTimeoutError`.
    ///
    /// Use this in a service handler to distinguish a message timeout (which may be waited
    /// out by calling `recv()` again) from other, non-resumable stream errors.
    pub fn find_in_chain<'a>(err: &'a (dyn StdError + 'static)) -> Option<&'a MessageTimeoutError> {
        let mut next: Option<&(dyn StdError + 'static)> = Some(err);
        while let Some(err) = next {
            if let Some(found) = err.downcast_ref::<MessageTimeoutError>() {
                return Some(found);
            }
            next = err.source();
        }
        None
    }
}

impl fmt::Display for MessageTimeoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitialMessage { timeout } => write!(
                f,
                "the RPC initial message didn't complete within the configured timeout ({:?}), including waiting for its first bytes",
                timeout
            ),
            Self::Idle { timeout } => write!(
                f,
                "an event stream remained idle waiting for the next message within the configured timeout ({:?})",
                timeout
            ),
            Self::MessageCompletion { timeout } => write!(
                f,
                "an event stream message started arriving, but didn't complete within the configured timeout ({:?})",
                timeout
            ),
        }
    }
}

impl StdError for MessageTimeoutError {}

/// Wraps `body` so that each event stream message frame must be fully read within `timeout`
/// of its first bytes being read.
#[cfg(test)]
pub(crate) fn wrap_with_message_timeout(body: SdkBody, timeout: Duration) -> SdkBody {
    wrap_with_event_stream_timeouts(body, None, Some(timeout), None, false)
}

/// Applies independent initial-read, event-completion, and idle deadlines.
///
/// For RPC protocols, `reads_initial_message` skips the per-event deadline on the first
/// frame, even when the initial deadline is disabled. REST streams have no initial read.
pub(crate) fn wrap_with_event_stream_timeouts(
    body: SdkBody,
    initial_timeout: Option<Duration>,
    message_timeout: Option<Duration>,
    idle_timeout: Option<Duration>,
    reads_initial_message: bool,
) -> SdkBody {
    if message_timeout.is_none()
        && idle_timeout.is_none()
        && (!reads_initial_message || initial_timeout.is_none())
    {
        return body;
    }
    let mut sleep = None;
    let mut armed = false;
    if reads_initial_message {
        if let Some(timeout) = initial_timeout {
            arm_deadline(&mut sleep, &mut armed, timeout);
        }
    } else if let Some(timeout) = idle_timeout {
        arm_deadline(&mut sleep, &mut armed, timeout);
    }
    SdkBody::from_body_1_x(MessageTimeoutBody {
        inner: body,
        initial_timeout,
        message_timeout,
        idle_timeout,
        reading_initial: reads_initial_message,
        frame_remaining: 0,
        prelude: [0; PRELUDE_SIZE],
        prelude_len: 0,
        sleep,
        armed,
    })
}

const PRELUDE_SIZE: usize = 4;

pin_project_lite::pin_project! {
    struct MessageTimeoutBody {
        #[pin]
        inner: SdkBody,
        initial_timeout: Option<Duration>,
        message_timeout: Option<Duration>,
        idle_timeout: Option<Duration>,
        reading_initial: bool,
        // Bytes of the current frame still expected after the length prefix.
        frame_remaining: u64,
        // The frame's 4-byte big-endian `total_length` prefix, collected across chunks.
        prelude: [u8; PRELUDE_SIZE],
        prelude_len: usize,
        // Timer allocated on the first enabled deadline and reused via `reset()` for every
        // deadline after that, so steady-state operation does not allocate per message.
        sleep: Option<Pin<Box<tokio::time::Sleep>>>,
        // Whether the current initial, completion, or idle deadline is armed.
        // Cleared after reporting a timeout, until the caller polls again.
        armed: bool,
    }
}

/// Arms a deadline `timeout` from now, reusing the existing timer when one was already
/// allocated.
fn arm_deadline(
    sleep: &mut Option<Pin<Box<tokio::time::Sleep>>>,
    armed: &mut bool,
    timeout: Duration,
) {
    let deadline = tokio::time::Instant::now() + timeout;
    match sleep {
        Some(sleep) => sleep.as_mut().reset(deadline),
        None => *sleep = Some(Box::pin(tokio::time::sleep_until(deadline))),
    }
    *armed = true;
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
        let mid_frame = *this.frame_remaining > 0 || *this.prelude_len > 0;
        let timeout = if *this.reading_initial {
            *this.initial_timeout
        } else if mid_frame {
            *this.message_timeout
        } else {
            *this.idle_timeout
        };
        if !*this.armed {
            // The caller resumed after a timeout: grant a new window before reading
            // anything, including data that is immediately ready.
            if let Some(timeout) = timeout {
                arm_deadline(this.sleep, this.armed, timeout);
            }
        }
        if *this.armed {
            let sleep = this
                .sleep
                .as_mut()
                .expect("a deadline is only armed after the timer is allocated");
            // Check the clock as well as polling Sleep: the timer driver may not have
            // observed exact or sub-millisecond expiry yet. Expiry takes priority over
            // ready data, leaving the inner body unread so a handler can retry.
            if tokio::time::Instant::now() >= sleep.deadline() || sleep.as_mut().poll(cx).is_ready()
            {
                *this.armed = false;
                let timeout = timeout.expect("an armed timer has a configured timeout");
                let error = if *this.reading_initial {
                    MessageTimeoutError::InitialMessage { timeout }
                } else if mid_frame {
                    MessageTimeoutError::MessageCompletion { timeout }
                } else {
                    MessageTimeoutError::Idle { timeout }
                };
                tracing::debug!(
                    timeout_millis = timeout.as_millis() as u64,
                    initial_message = error.is_initial_message(),
                    idle = error.is_idle(),
                    "event stream message read timed out"
                );
                return Poll::Ready(Some(Err(Box::new(error))));
            }
        }
        match this.inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    let mut tracker = FrameTracker {
                        frame_remaining: this.frame_remaining,
                        prelude: this.prelude,
                        prelude_len: this.prelude_len,
                    };
                    let completed = tracker.note_bytes(data);
                    if completed {
                        *this.armed = false;
                        *this.reading_initial = false;
                    }
                    if !*this.reading_initial {
                        if tracker.is_mid_frame() && (!mid_frame || completed) {
                            // First bytes end the idle phase; subsequent bytes never reset
                            // the completion deadline, so slow drips cannot extend it.
                            *this.armed = false;
                            if let Some(timeout) = *this.message_timeout {
                                arm_deadline(this.sleep, this.armed, timeout);
                            }
                        } else if !tracker.is_mid_frame() && completed {
                            if let Some(timeout) = *this.idle_timeout {
                                arm_deadline(this.sleep, this.armed, timeout);
                            }
                        }
                    }
                }
                Poll::Ready(Some(Ok(frame)))
            }
            other => other,
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
    use super::{wrap_with_event_stream_timeouts, wrap_with_message_timeout, MessageTimeoutError};
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

    fn rpc_body(steps: Vec<Step>, initial: Option<u64>, message: Option<u64>) -> SdkBody {
        wrap_with_event_stream_timeouts(
            SdkBody::from_body_1_x(StepBody::new(steps)),
            initial.map(Duration::from_millis),
            message.map(Duration::from_millis),
            None,
            true,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn initial_deadline_covers_a_sender_that_sends_no_bytes() {
        use http_body_util::BodyExt;
        let start = tokio::time::Instant::now();
        let mut body = rpc_body(vec![Step::Hang], Some(100), Some(10));
        let error = body.frame().await.unwrap().unwrap_err();
        let error = MessageTimeoutError::find_in_chain(error.as_ref()).unwrap();
        assert!(error.is_initial_message());
        assert!(matches!(error, MessageTimeoutError::InitialMessage { .. }));
        assert_eq!(error.timeout(), Duration::from_millis(100));
        assert_eq!(start.elapsed(), Duration::from_millis(100));
    }

    #[tokio::test(start_paused = true)]
    async fn initial_deadline_is_not_reset_when_first_bytes_arrive() {
        use http_body_util::BodyExt;
        let start = tokio::time::Instant::now();
        let mut body = rpc_body(
            vec![
                Step::DelayThenData(Duration::from_millis(60), encode_frame(20).slice(..1)),
                Step::Data(Bytes::new()),
                Step::Hang,
            ],
            Some(100),
            Some(10),
        );
        assert!(body.frame().await.unwrap().is_ok());
        assert!(body.frame().await.unwrap().is_ok());
        let error = body.frame().await.unwrap().unwrap_err();
        assert!(MessageTimeoutError::find_in_chain(error.as_ref())
            .unwrap()
            .is_initial_message());
        assert_eq!(start.elapsed(), Duration::from_millis(100));
    }

    #[tokio::test(start_paused = true)]
    async fn expired_initial_deadline_precedes_ready_first_bytes() {
        let mut body = rpc_body(vec![Step::Data(encode_frame(20))], Some(100), Some(10));
        tokio::time::advance(Duration::from_millis(100)).await;
        let mut cx = Context::from_waker(std::task::Waker::noop());
        match Pin::new(&mut body).poll_frame(&mut cx) {
            Poll::Ready(Some(Err(error))) => {
                assert!(MessageTimeoutError::find_in_chain(error.as_ref())
                    .unwrap()
                    .is_initial_message());
            }
            other => panic!("initial deadline must win over ready data: {other:?}"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn initial_and_event_windows_are_independent() {
        use http_body_util::BodyExt;
        let initial = encode_frame(20);
        let event = encode_frame(20);
        let start = tokio::time::Instant::now();
        let mut body = rpc_body(
            vec![
                Step::Data(initial.slice(..6)),
                Step::DelayThenData(Duration::from_millis(200), initial.slice(6..)),
                Step::DelayThenData(Duration::from_millis(1000), event.slice(..6)),
                Step::Hang,
            ],
            Some(500),
            Some(20),
        );
        for _ in 0..3 {
            assert!(body.frame().await.unwrap().is_ok());
        }
        let error = body.frame().await.unwrap().unwrap_err();
        let error = MessageTimeoutError::find_in_chain(error.as_ref()).unwrap();
        assert!(!error.is_initial_message());
        assert!(matches!(
            error,
            MessageTimeoutError::MessageCompletion { .. }
        ));
        assert_eq!(error.timeout(), Duration::from_millis(20));
        assert_eq!(start.elapsed(), Duration::from_millis(1220));
    }

    #[tokio::test(start_paused = true)]
    async fn initial_boundary_chunk_arms_the_event_window() {
        use http_body_util::BodyExt;
        let initial = encode_frame(20);
        let event = encode_frame(20);
        let mut combined = initial.to_vec();
        combined.extend_from_slice(&event.slice(..6));
        let start = tokio::time::Instant::now();
        let mut body = rpc_body(
            vec![Step::Data(combined.into()), Step::Hang],
            Some(500),
            Some(20),
        );
        assert!(body.frame().await.unwrap().is_ok());
        let error = body.frame().await.unwrap().unwrap_err();
        assert!(!MessageTimeoutError::find_in_chain(error.as_ref())
            .unwrap()
            .is_initial_message());
        assert_eq!(start.elapsed(), Duration::from_millis(20));
    }

    #[tokio::test(start_paused = true)]
    async fn either_rpc_deadline_can_be_disabled_independently() {
        use http_body_util::BodyExt;
        let initial = encode_frame(20);
        let event = encode_frame(20);
        let mut body = rpc_body(
            vec![
                Step::Data(initial.slice(..6)),
                Step::DelayThenData(Duration::from_secs(3600), initial.slice(6..)),
                Step::Data(event.slice(..6)),
                Step::Hang,
            ],
            None,
            Some(20),
        );
        for _ in 0..3 {
            assert!(body.frame().await.unwrap().is_ok());
        }
        let error = body.frame().await.unwrap().unwrap_err();
        assert!(!MessageTimeoutError::find_in_chain(error.as_ref())
            .unwrap()
            .is_initial_message());
        let body = rpc_body(
            vec![
                Step::Data(initial),
                Step::Data(event.slice(..6)),
                Step::DelayThenData(Duration::from_secs(3600), event.slice(6..)),
            ],
            Some(20),
            None,
        );
        assert_eq!(collect_body(body).await.unwrap().len(), 3);
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
    async fn completion_timeout_does_not_apply_between_messages_when_idle_timeout_disabled() {
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

    fn idle_body(
        steps: Vec<Step>,
        initial: Option<u64>,
        message: Option<u64>,
        idle: Option<u64>,
        rpc: bool,
    ) -> SdkBody {
        wrap_with_event_stream_timeouts(
            SdkBody::from_body_1_x(StepBody::new(steps)),
            initial.map(Duration::from_millis),
            message.map(Duration::from_millis),
            idle.map(Duration::from_millis),
            rpc,
        )
    }

    #[tokio::test(start_paused = true)]
    async fn idle_timeout_bounds_rest_first_event_and_gaps_after_complete_frames() {
        use http_body_util::BodyExt;
        for rpc in [false, true] {
            let mut body = idle_body(
                vec![Step::Data(encode_frame(20)), Step::Hang],
                Some(10),
                None,
                Some(3_600_000),
                rpc,
            );
            body.frame().await.unwrap().unwrap();
            let start = tokio::time::Instant::now();
            let error = body.frame().await.unwrap().unwrap_err();
            let error = MessageTimeoutError::find_in_chain(error.as_ref()).unwrap();
            assert!(error.is_idle());
            assert!(matches!(error, MessageTimeoutError::Idle { .. }));
            assert!(!error.is_initial_message());
            assert_eq!(error.timeout(), Duration::from_secs(3600));
            assert_eq!(start.elapsed(), Duration::from_secs(3600));
        }
        let mut body = idle_body(vec![Step::Hang], None, None, Some(100), false);
        let error = body.frame().await.unwrap().unwrap_err();
        assert!(MessageTimeoutError::find_in_chain(error.as_ref())
            .unwrap()
            .is_idle());
    }

    #[tokio::test(start_paused = true)]
    async fn first_bytes_switch_idle_deadline_to_completion_without_extending_slow_drips() {
        use http_body_util::BodyExt;
        let frame = encode_frame(20);
        let mut body = idle_body(
            vec![
                Step::DelayThenData(Duration::from_millis(90), frame.slice(..1)),
                Step::DelayThenData(Duration::from_millis(15), frame.slice(1..2)),
                Step::Hang,
            ],
            None,
            Some(20),
            Some(100),
            false,
        );
        let start = tokio::time::Instant::now();
        body.frame().await.unwrap().unwrap();
        body.frame().await.unwrap().unwrap();
        let error = body.frame().await.unwrap().unwrap_err();
        let error = MessageTimeoutError::find_in_chain(error.as_ref()).unwrap();
        assert!(!error.is_idle());
        assert_eq!(error.timeout(), Duration::from_millis(20));
        assert_eq!(start.elapsed(), Duration::from_millis(110));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_deadline_resets_after_each_complete_frame_and_ignores_empty_chunks() {
        use http_body_util::BodyExt;
        let mut body = idle_body(
            vec![
                Step::DelayThenData(Duration::from_millis(90), encode_frame(20)),
                Step::DelayThenData(Duration::from_millis(90), encode_frame(20)),
                Step::DelayThenData(Duration::from_millis(90), Bytes::new()),
                Step::Hang,
            ],
            None,
            None,
            Some(100),
            false,
        );
        let start = tokio::time::Instant::now();
        for _ in 0..3 {
            body.frame().await.unwrap().unwrap();
        }
        let error = body.frame().await.unwrap().unwrap_err();
        assert!(MessageTimeoutError::find_in_chain(error.as_ref())
            .unwrap()
            .is_idle());
        assert_eq!(start.elapsed(), Duration::from_millis(280));
        // Retrying an idle timeout grants one new idle window, without losing the body.
        let error = body.frame().await.unwrap().unwrap_err();
        assert!(MessageTimeoutError::find_in_chain(error.as_ref())
            .unwrap()
            .is_idle());
        assert_eq!(start.elapsed(), Duration::from_millis(380));
    }

    #[tokio::test(start_paused = true)]
    async fn idle_timeout_does_not_shorten_initial_or_disabled_completion_deadline() {
        use http_body_util::BodyExt;
        let mut body = idle_body(vec![Step::Hang], Some(100), Some(10), Some(20), true);
        let error = body.frame().await.unwrap().unwrap_err();
        let error = MessageTimeoutError::find_in_chain(error.as_ref()).unwrap();
        assert!(error.is_initial_message());
        assert!(!error.is_idle());
        assert_eq!(error.timeout(), Duration::from_millis(100));

        let frame = encode_frame(20);
        let body = idle_body(
            vec![
                Step::Data(frame.slice(..1)),
                Step::DelayThenData(Duration::from_millis(1000), frame.slice(1..)),
            ],
            None,
            None,
            Some(20),
            false,
        );
        collect_body(body)
            .await
            .expect("completion deadline is disabled");
    }

    #[tokio::test(start_paused = true)]
    async fn expired_idle_deadline_precedes_ready_data_and_preserves_it_for_retry() {
        use http_body_util::BodyExt;
        for delay in [
            Duration::from_millis(100),
            Duration::from_millis(100) + Duration::from_micros(1),
            Duration::from_millis(200),
        ] {
            let frame = encode_frame(20);
            let mut body = idle_body(
                vec![Step::Data(frame.clone())],
                None,
                Some(20),
                Some(100),
                false,
            );
            tokio::time::advance(delay).await;
            let error = body.frame().await.unwrap().unwrap_err();
            assert!(MessageTimeoutError::find_in_chain(error.as_ref())
                .unwrap()
                .is_idle());
            assert_eq!(
                body.frame().await.unwrap().unwrap().into_data().unwrap(),
                frame,
                "idle expiry must leave ready bytes available to the retry"
            );
            assert!(body.frame().await.is_none());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn boundary_chunk_switches_completion_to_idle_only_after_the_last_frame_completes() {
        use http_body_util::BodyExt;
        for rpc in [false, true] {
            let first = encode_frame(20);
            let second = encode_frame(20);
            let mut boundary = first.to_vec();
            boundary.extend_from_slice(&second[..2]);
            let mut body = idle_body(
                vec![
                    Step::Data(boundary.into()),
                    Step::DelayThenData(Duration::from_millis(15), second.slice(2..)),
                    Step::Hang,
                ],
                Some(10),
                Some(20),
                Some(100),
                rpc,
            );
            let start = tokio::time::Instant::now();
            body.frame().await.unwrap().unwrap();
            body.frame().await.unwrap().unwrap();
            let error = body.frame().await.unwrap().unwrap_err();
            assert!(MessageTimeoutError::find_in_chain(error.as_ref())
                .unwrap()
                .is_idle());
            assert_eq!(start.elapsed(), Duration::from_millis(115));

            // Stalling in the second frame uses its completion window, not the idle
            // window or (for RPC) the first frame's initial-read window.
            let mut boundary = first.to_vec();
            boundary.extend_from_slice(&second[..2]);
            let mut body = idle_body(
                vec![Step::Data(boundary.into()), Step::Hang],
                Some(10),
                Some(20),
                Some(100),
                rpc,
            );
            let start = tokio::time::Instant::now();
            body.frame().await.unwrap().unwrap();
            let error = body.frame().await.unwrap().unwrap_err();
            let error = MessageTimeoutError::find_in_chain(error.as_ref()).unwrap();
            assert!(!error.is_idle());
            assert!(!error.is_initial_message());
            assert_eq!(start.elapsed(), Duration::from_millis(20));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn end_of_stream_during_idle_finishes_without_waiting_for_deadline() {
        use http_body_util::BodyExt;
        for steps in [vec![], vec![Step::Data(encode_frame(20))]] {
            let body = idle_body(steps, None, Some(20), Some(100), false);
            let start = tokio::time::Instant::now();
            collect_body(body)
                .await
                .expect("clean EOF is not a timeout");
            assert_eq!(start.elapsed(), Duration::ZERO);
        }
        let mut body = idle_body(vec![], None, None, None, false);
        assert!(body.frame().await.is_none());
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
    async fn expired_deadline_precedes_ready_data_and_preserves_it_for_retry() {
        use super::MessageTimeoutError;
        use http_body_util::BodyExt;

        // Cover a split length prefix and a split frame, including exact expiry and a
        // sub-millisecond overrun before the timer driver necessarily observes expiry.
        for split in [2, 10] {
            for delay in [
                Duration::from_millis(100),
                Duration::from_millis(100) + Duration::from_micros(1),
                Duration::from_millis(200),
            ] {
                let frame = encode_frame(20);
                let first = frame.slice(..split);
                let remaining = frame.slice(split..);
                let mut body = wrapped_body(
                    vec![Step::Data(first.clone()), Step::Data(remaining.clone())],
                    Duration::from_millis(100),
                );
                assert_eq!(
                    body.frame().await.unwrap().unwrap().into_data().unwrap(),
                    first
                );
                tokio::time::advance(delay).await;

                let err = body
                    .frame()
                    .await
                    .unwrap()
                    .expect_err("expiry must win even when the next chunk is ready");
                assert!(MessageTimeoutError::find_in_chain(err.as_ref()).is_some());
                assert_eq!(
                    body.frame().await.unwrap().unwrap().into_data().unwrap(),
                    remaining,
                    "retry must receive the chunk that the timeout left unread"
                );
                assert!(body.frame().await.is_none());
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn ready_data_before_deadline_completes_without_timeout() {
        use http_body_util::BodyExt;

        let frame = encode_frame(20);
        let remaining = frame.slice(10..);
        let mut body = wrapped_body(
            vec![Step::Data(frame.slice(..10)), Step::Data(remaining.clone())],
            Duration::from_millis(100),
        );
        body.frame().await.unwrap().unwrap();
        tokio::time::advance(Duration::from_millis(99)).await;
        assert_eq!(
            body.frame().await.unwrap().unwrap().into_data().unwrap(),
            remaining
        );
        assert!(body.frame().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn completed_message_timer_does_not_expire_the_next_message_after_idle() {
        use http_body_util::BodyExt;

        let frame = encode_frame(20);
        let mut body = wrapped_body(
            vec![
                Step::Data(frame.slice(..10)),
                Step::Data(frame.slice(10..)),
                Step::Data(frame.slice(..10)),
                Step::Data(frame.slice(10..)),
            ],
            Duration::from_millis(100),
        );
        body.frame().await.unwrap().unwrap();
        tokio::time::advance(Duration::from_millis(50)).await;
        body.frame().await.unwrap().unwrap();
        tokio::time::advance(Duration::from_secs(3600)).await;
        body.frame().await.unwrap().unwrap();
        tokio::time::advance(Duration::from_millis(99)).await;
        assert_eq!(
            body.frame().await.unwrap().unwrap().into_data().unwrap(),
            frame.slice(10..),
            "the new message must get a fresh window after the idle gap"
        );
        assert!(body.frame().await.is_none());
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

    /// Like `collect_body`, but keeps polling after errors, recording each outcome. Models a
    /// handler that chooses to keep waiting after a message timeout.
    async fn collect_body_through_errors(mut body: SdkBody) -> Vec<Result<Bytes, String>> {
        use http_body_util::BodyExt;
        let mut outcomes = Vec::new();
        while let Some(frame) = body.frame().await {
            match frame {
                Ok(frame) => {
                    if let Ok(data) = frame.into_data() {
                        outcomes.push(Ok(data));
                    }
                }
                Err(err) => outcomes.push(Err(err.to_string())),
            }
        }
        outcomes
    }

    #[tokio::test(start_paused = true)]
    async fn stream_resumes_when_polled_again_after_timeout() {
        let frame = encode_frame(20);
        let len = frame.len();
        let body = wrapped_body(
            vec![
                Step::Data(frame.slice(0..10)),
                // Arrives at 250ms: two 100ms windows expire first (one initial, one
                // granted by continuing), then the data lands within the third window.
                Step::DelayThenData(Duration::from_millis(250), frame.slice(10..len)),
            ],
            Duration::from_millis(100),
        );
        let outcomes = collect_body_through_errors(body).await;
        let timeouts = outcomes.iter().filter(|o| o.is_err()).count();
        let data: Vec<_> = outcomes.iter().filter(|o| o.is_ok()).collect();
        assert_eq!(2, timeouts, "{outcomes:?}");
        assert_eq!(2, data.len(), "{outcomes:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn timeout_remains_terminal_when_error_is_propagated() {
        // A handler that treats the first timeout as fatal (the default `?` path) never
        // polls again; the single error it saw is the hard-reject behavior.
        let frame = encode_frame(20);
        let body = wrapped_body(
            vec![Step::Data(frame.slice(0..10)), Step::Hang],
            Duration::from_millis(100),
        );
        collect_body(body).await.expect_err("expected a timeout");
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
