/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Provides Sender/Receiver implementations for Event Stream codegen.

use crate::body::SdkBody;
use crate::result::SdkError;
use bytes::Buf;
use bytes::Bytes;
use bytes_utils::SegmentedBuf;
use futures_core::Stream;
use hyper::body::HttpBody;
use pin_project::pin_project;
use smithy_eventstream::frame::{
    DecodedFrame, MarshallMessage, Message, MessageFrameDecoder, SignMessage, UnmarshallMessage,
    UnmarshalledMessage,
};
use std::error::Error as StdError;
use std::fmt;
use std::marker::PhantomData;
use std::mem;
use std::pin::Pin;
use std::task::{Context, Poll};

pub type BoxError = Box<dyn StdError + Send + Sync + 'static>;

/// Input type for Event Streams.
pub struct EventStreamInput<T> {
    input_stream: Pin<Box<dyn Stream<Item = Result<T, BoxError>> + Send>>,
}

impl<T> fmt::Debug for EventStreamInput<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EventStreamInput(Box<dyn Stream>)")
    }
}

impl<T> EventStreamInput<T> {
    #[doc(hidden)]
    pub fn into_body_stream<E: StdError + Send + Sync + 'static>(
        self,
        marshaller: impl MarshallMessage<Input = T> + Send + Sync + 'static,
        signer: impl SignMessage + Send + Sync + 'static,
    ) -> MessageStreamAdapter<T, E> {
        MessageStreamAdapter::new(marshaller, signer, self.input_stream)
    }
}

impl<T, S> From<S> for EventStreamInput<T>
where
    S: Stream<Item = Result<T, BoxError>> + Send + 'static,
{
    fn from(stream: S) -> Self {
        EventStreamInput {
            input_stream: Box::pin(stream),
        }
    }
}

/// Adapts a `Stream<SmithyMessageType>` to a signed `Stream<Bytes>` by using the provided
/// message marshaller and signer implementations.
///
/// This will yield an `Err(SdkError::ConstructionFailure)` if a message can't be
/// marshalled into an Event Stream frame, (e.g., if the message payload was too large).
#[pin_project]
pub struct MessageStreamAdapter<T, E> {
    marshaller: Box<dyn MarshallMessage<Input = T> + Send + Sync>,
    signer: Box<dyn SignMessage + Send + Sync>,
    #[pin]
    stream: Pin<Box<dyn Stream<Item = Result<T, BoxError>> + Send>>,
    end_signal_sent: bool,
    _phantom: PhantomData<E>,
}

impl<T, E> MessageStreamAdapter<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    pub fn new(
        marshaller: impl MarshallMessage<Input = T> + Send + Sync + 'static,
        signer: impl SignMessage + Send + Sync + 'static,
        stream: Pin<Box<dyn Stream<Item = Result<T, BoxError>> + Send>>,
    ) -> Self {
        MessageStreamAdapter {
            marshaller: Box::new(marshaller),
            signer: Box::new(signer),
            stream,
            end_signal_sent: false,
            _phantom: Default::default(),
        }
    }
}

impl<T, E> Stream for MessageStreamAdapter<T, E>
where
    E: StdError + Send + Sync + 'static,
{
    type Item = Result<Bytes, SdkError<E>>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match this.stream.poll_next(cx) {
            Poll::Ready(message_option) => {
                if let Some(message_result) = message_option {
                    let message_result =
                        message_result.map_err(|err| SdkError::ConstructionFailure(err));
                    let message = this
                        .marshaller
                        .marshall(message_result?)
                        .map_err(|err| SdkError::ConstructionFailure(Box::new(err)))?;
                    let message = this
                        .signer
                        .sign(message)
                        .map_err(|err| SdkError::ConstructionFailure(err))?;
                    let mut buffer = Vec::new();
                    message
                        .write_to(&mut buffer)
                        .map_err(|err| SdkError::ConstructionFailure(Box::new(err)))?;
                    Poll::Ready(Some(Ok(Bytes::from(buffer))))
                } else if !*this.end_signal_sent {
                    *this.end_signal_sent = true;
                    let mut buffer = Vec::new();
                    this.signer
                        .sign_empty()
                        .map_err(|err| SdkError::ConstructionFailure(err))?
                        .write_to(&mut buffer)
                        .map_err(|err| SdkError::ConstructionFailure(Box::new(err)))?;
                    Poll::Ready(Some(Ok(Bytes::from(buffer))))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Wrapper around SegmentedBuf that tracks the state of the stream.
#[derive(Debug)]
enum RecvBuf {
    /// Nothing has been buffered yet.
    Empty,
    /// Some data has been buffered.
    /// The SegmentedBuf will automatically purge when it reads off the end of a chunk boundary
    Partial(SegmentedBuf<Bytes>),
    /// The end of the stream has been reached, but there may still be some buffered data.
    EosPartial(SegmentedBuf<Bytes>),
}

impl RecvBuf {
    fn has_data(&self) -> bool {
        match self {
            RecvBuf::Empty => false,
            RecvBuf::Partial(segments) | RecvBuf::EosPartial(segments) => segments.remaining(),
        }
    }

    fn is_eos(&self) -> bool {
        matches!(self, RecvBuf::EosPartial(_))
    }

    fn buffered(&mut self) -> &mut SegmentedBuf<Bytes> {
        match self {
            RecvBuf::Empty => panic!("buffer must be populated before reading; this is a bug"),
            RecvBuf::Partial(segmented) => segmented,
            RecvBuf::EosPartial(segmented) => segmented,
        }
    }

    fn with_partial(self, partial: Bytes) -> Self {
        match self {
            RecvBuf::Empty => {
                let mut segmented = SegmentedBuf::new();
                segmented.push(partial);
                RecvBuf::Partial(segmented)
            }
            RecvBuf::Partial(mut segmented) => {
                segmented.push(partial);
                RecvBuf::Partial(segmented)
            }
            RecvBuf::EosPartial(_) => {
                panic!("cannot buffer more data after the stream has ended; this is a bug")
            }
        }
    }

    fn ended(self) -> Self {
        match self {
            RecvBuf::Empty => RecvBuf::EosPartial(SegmentedBuf::new()),
            RecvBuf::Partial(segmented) => RecvBuf::EosPartial(segmented),
            RecvBuf::EosPartial(_) => panic!("already end of stream; this is a bug"),
        }
    }
}

/// Raw message from a [`Receiver`] when a [`SdkError::ResponseError`] is returned.
#[derive(Debug)]
#[non_exhaustive]
pub enum RawMessage {
    /// Message was decoded into a valid frame, but failed to unmarshall into a modeled type.
    Decoded(Message),
    /// Message failed to be decoded into a valid frame. The raw bytes may not be available in the
    /// case where decoding consumed the buffer.
    Invalid(Option<Bytes>),
}

impl From<&mut SegmentedBuf<Bytes>> for RawMessage {
    fn from(buf: &mut SegmentedBuf<Bytes>) -> Self {
        Self::Invalid(Some(buf.copy_to_bytes(buf.remaining())))
    }
}

#[derive(Debug)]
pub enum Error {
    /// The stream ended before a complete message frame was received.
    UnexpectedEndOfStream,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEndOfStream => write!(f, "unexpected end of stream"),
        }
    }
}

impl StdError for Error {}

/// Receives Smithy-modeled messages out of an Event Stream.
#[derive(Debug)]
pub struct Receiver<T, E> {
    unmarshaller: Box<dyn UnmarshallMessage<Output = T, Error = E> + Send>,
    decoder: MessageFrameDecoder,
    buffer: RecvBuf,
    body: SdkBody,
    /// Event Stream has optional initial response frames an with `:message-type` of
    /// `initial-response`. If `try_recv_initial()` is called and the next message isn't an
    /// initial response, then the message will be stored in `buffered_message` so that it can
    /// be returned with the next call of `recv()`.
    buffered_message: Option<Message>,
    _phantom: PhantomData<E>,
}

impl<T, E> Receiver<T, E> {
    /// Creates a new `Receiver` with the given message unmarshaller and SDK body.
    pub fn new(
        unmarshaller: impl UnmarshallMessage<Output = T, Error = E> + Send + 'static,
        body: SdkBody,
    ) -> Self {
        Receiver {
            unmarshaller: Box::new(unmarshaller),
            decoder: MessageFrameDecoder::new(),
            buffer: RecvBuf::Empty,
            body,
            buffered_message: None,
            _phantom: Default::default(),
        }
    }

    fn unmarshall(&self, message: Message) -> Result<Option<T>, SdkError<E, RawMessage>> {
        match self.unmarshaller.unmarshall(&message) {
            Ok(unmarshalled) => match unmarshalled {
                UnmarshalledMessage::Event(event) => Ok(Some(event)),
                UnmarshalledMessage::Error(err) => Err(SdkError::ServiceError {
                    err,
                    raw: RawMessage::Decoded(message),
                }),
            },
            Err(err) => Err(SdkError::ResponseError {
                err: Box::new(err),
                raw: RawMessage::Decoded(message),
            }),
        }
    }

    async fn buffer_next_chunk(&mut self) -> Result<(), SdkError<E, RawMessage>> {
        if !self.buffer.is_eos() {
            let next_chunk = self
                .body
                .data()
                .await
                .transpose()
                .map_err(|err| SdkError::DispatchFailure(err))?;
            let buffer = mem::replace(&mut self.buffer, RecvBuf::Empty);
            if let Some(chunk) = next_chunk {
                self.buffer = buffer.with_partial(chunk);
            } else {
                self.buffer = buffer.ended();
            }
        }
        Ok(())
    }

    async fn next_message(&mut self) -> Result<Option<Message>, SdkError<E, RawMessage>> {
        while !self.buffer.is_eos() {
            if self.buffer.has_data() {
                if let DecodedFrame::Complete(message) = self
                    .decoder
                    .decode_frame(self.buffer.buffered())
                    .map_err(|err| SdkError::ResponseError {
                        err: Box::new(err),
                        raw: RawMessage::Invalid(None), // the buffer has been consumed
                    })?
                {
                    return Ok(Some(message));
                }
            }

            self.buffer_next_chunk().await?;
        }
        if self.buffer.has_data() {
            return Err(SdkError::ResponseError {
                err: Error::UnexpectedEndOfStream.into(),
                raw: self.buffer.buffered().into(),
            });
        }
        Ok(None)
    }

    /// Tries to receive the initial response message that has `:event-type` of `initial-response`.
    /// If a different event type is received, then it is buffered and `Ok(None)` is returned.
    #[doc(hidden)]
    pub async fn try_recv_initial(&mut self) -> Result<Option<Message>, SdkError<E, RawMessage>> {
        if let Some(message) = self.next_message().await? {
            if let Some(event_type) = message
                .headers()
                .iter()
                .find(|h| h.name().as_str() == ":event-type")
            {
                if event_type
                    .value()
                    .as_string()
                    .map(|s| s.as_str() == "initial-response")
                    .unwrap_or(false)
                {
                    return Ok(Some(message));
                }
            } else {
                // Buffer the message so that it can be returned by the next call to `recv()`
                self.buffered_message = Some(message);
            }
        }
        Ok(None)
    }

    /// Asynchronously tries to receive a message from the stream. If the stream has ended,
    /// it returns an `Ok(None)`. If there is a transport layer error, it will return
    /// `Err(SdkError::DispatchFailure)`. Service-modeled errors will be a part of the returned
    /// messages.
    pub async fn recv(&mut self) -> Result<Option<T>, SdkError<E, RawMessage>> {
        if let Some(buffered) = self.buffered_message.take() {
            return self.unmarshall(buffered);
        }
        if let Some(message) = self.next_message().await? {
            self.unmarshall(message)
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MarshallMessage, Receiver, UnmarshallMessage};
    use crate::body::SdkBody;
    use crate::event_stream::{EventStreamInput, MessageStreamAdapter};
    use crate::result::SdkError;
    use async_stream::stream;
    use bytes::Bytes;
    use futures_core::Stream;
    use futures_util::stream::StreamExt;
    use hyper::body::Body;
    use smithy_eventstream::error::Error as EventStreamError;
    use smithy_eventstream::frame::{
        Header, HeaderValue, Message, SignMessage, SignMessageError, UnmarshalledMessage,
    };
    use std::error::Error as StdError;
    use std::io::{Error as IOError, ErrorKind};

    fn encode_initial_response() -> Bytes {
        let mut buffer = Vec::new();
        Message::new(Bytes::new())
            .add_header(Header::new(
                ":message-type",
                HeaderValue::String("event".into()),
            ))
            .add_header(Header::new(
                ":event-type",
                HeaderValue::String("initial-response".into()),
            ))
            .write_to(&mut buffer)
            .unwrap();
        buffer.into()
    }

    fn encode_message(message: &str) -> Bytes {
        let mut buffer = Vec::new();
        Message::new(Bytes::copy_from_slice(message.as_bytes()))
            .write_to(&mut buffer)
            .unwrap();
        buffer.into()
    }

    #[derive(Debug)]
    struct FakeError;
    impl std::fmt::Display for FakeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "FakeError")
        }
    }
    impl StdError for FakeError {}

    #[derive(Debug, Eq, PartialEq)]
    struct TestMessage(String);

    #[derive(Debug)]
    struct Marshaller;
    impl MarshallMessage for Marshaller {
        type Input = TestMessage;

        fn marshall(&self, input: Self::Input) -> Result<Message, EventStreamError> {
            Ok(Message::new(input.0.as_bytes().to_vec()))
        }
    }

    #[derive(Debug)]
    struct Unmarshaller;
    impl UnmarshallMessage for Unmarshaller {
        type Output = TestMessage;
        type Error = EventStreamError;

        fn unmarshall(
            &self,
            message: &Message,
        ) -> Result<UnmarshalledMessage<Self::Output, Self::Error>, EventStreamError> {
            Ok(UnmarshalledMessage::Event(TestMessage(
                std::str::from_utf8(&message.payload()[..]).unwrap().into(),
            )))
        }
    }

    #[tokio::test]
    async fn receive_success() {
        let chunks: Vec<Result<_, IOError>> =
            vec![Ok(encode_message("one")), Ok(encode_message("two"))];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            TestMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert_eq!(
            TestMessage("two".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert_eq!(None, receiver.recv().await.unwrap());
    }

    #[tokio::test]
    async fn receive_last_chunk_empty() {
        let chunks: Vec<Result<_, IOError>> = vec![
            Ok(encode_message("one")),
            Ok(encode_message("two")),
            Ok(Bytes::from_static(&[])),
        ];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            TestMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert_eq!(
            TestMessage("two".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert_eq!(None, receiver.recv().await.unwrap());
    }

    #[tokio::test]
    async fn receive_last_chunk_not_full_message() {
        let chunks: Vec<Result<_, IOError>> = vec![
            Ok(encode_message("one")),
            Ok(encode_message("two")),
            Ok(encode_message("three").split_to(10)),
        ];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            TestMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert_eq!(
            TestMessage("two".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert!(matches!(
            receiver.recv().await,
            Err(SdkError::ResponseError { .. }),
        ));
    }

    proptest::proptest! {
        #[test]
        fn receive_multiple_messages_split_unevenly_across_chunks(b1: usize, b2: usize) {
            let msg1 = encode_message("one");
            let msg2 = encode_message("two");
            let msg3 = encode_message("three");
            let msg4 = encode_message("four");
            let combined = {
                let mut combined = Vec::new();
                combined.extend_from_slice(&msg1);
                combined.extend_from_slice(&msg2);
                combined.extend_from_slice(&msg3);
                combined.extend_from_slice(&msg4);
                combined
            };

            let midpoint = combined.len() / 2;
            let (start, boundary1, boundary2, end) = (
                0,
                b1 % midpoint,
                midpoint + b2 % midpoint,
                combined.len()
            );
            println!("[{}, {}], [{}, {}], [{}, {}]", start, boundary1, boundary1, boundary2, boundary2, end);

            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let chunks: Vec<Result<_, IOError>> = vec![
                    Ok(Bytes::copy_from_slice(&combined[start..boundary1])),
                    Ok(Bytes::copy_from_slice(&combined[boundary1..boundary2])),
                    Ok(Bytes::copy_from_slice(&combined[boundary2..end])),
                ];

                let chunk_stream = futures_util::stream::iter(chunks);
                let body = SdkBody::from(Body::wrap_stream(chunk_stream));
                let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
                for payload in &["one", "two", "three", "four"] {
                    assert_eq!(
                        TestMessage((*payload).into()),
                        receiver.recv().await.unwrap().unwrap()
                    );
                }
                assert_eq!(None, receiver.recv().await.unwrap());
            });
        }
    }

    #[tokio::test]
    async fn receive_network_failure() {
        let chunks: Vec<Result<_, IOError>> = vec![
            Ok(encode_message("one")),
            Err(IOError::new(ErrorKind::ConnectionReset, FakeError)),
        ];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            TestMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert!(matches!(
            receiver.recv().await,
            Err(SdkError::DispatchFailure(_))
        ));
    }

    #[tokio::test]
    async fn receive_message_parse_failure() {
        let chunks: Vec<Result<_, IOError>> = vec![
            Ok(encode_message("one")),
            // A zero length message will be invalid. We need to provide a minimum of 12 bytes
            // for the MessageFrameDecoder to actually start parsing it.
            Ok(Bytes::from_static(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])),
        ];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            TestMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert!(matches!(
            receiver.recv().await,
            Err(SdkError::ResponseError { .. })
        ));
    }

    #[tokio::test]
    async fn receive_initial_response() {
        let chunks: Vec<Result<_, IOError>> =
            vec![Ok(encode_initial_response()), Ok(encode_message("one"))];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
        assert!(receiver.try_recv_initial().await.unwrap().is_some());
        assert_eq!(
            TestMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
    }

    #[tokio::test]
    async fn receive_no_initial_response() {
        let chunks: Vec<Result<_, IOError>> =
            vec![Ok(encode_message("one")), Ok(encode_message("two"))];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver = Receiver::<TestMessage, EventStreamError>::new(Unmarshaller, body);
        assert!(receiver.try_recv_initial().await.unwrap().is_none());
        assert_eq!(
            TestMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert_eq!(
            TestMessage("two".into()),
            receiver.recv().await.unwrap().unwrap()
        );
    }

    fn assert_send<T: Send>() {}

    #[tokio::test]
    async fn receiver_is_send() {
        assert_send::<Receiver<(), ()>>();
    }

    #[derive(Debug)]
    struct TestServiceError;
    impl std::fmt::Display for TestServiceError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestServiceError")
        }
    }
    impl StdError for TestServiceError {}

    #[derive(Debug)]
    struct TestSigner;
    impl SignMessage for TestSigner {
        fn sign(&mut self, message: Message) -> Result<Message, SignMessageError> {
            let mut buffer = Vec::new();
            message.write_to(&mut buffer).unwrap();
            Ok(Message::new(buffer).add_header(Header::new("signed", HeaderValue::Bool(true))))
        }

        fn sign_empty(&mut self) -> Result<Message, SignMessageError> {
            Ok(Message::new(&b""[..]).add_header(Header::new("signed", HeaderValue::Bool(true))))
        }
    }

    fn check_compatible_with_hyper_wrap_stream<S, O, E>(stream: S) -> S
    where
        S: Stream<Item = Result<O, E>> + Send + 'static,
        O: Into<Bytes> + 'static,
        E: Into<Box<dyn StdError + Send + Sync>> + 'static,
    {
        stream
    }

    #[tokio::test]
    async fn message_stream_adapter_success() {
        let stream = stream! {
            yield Ok(TestMessage("test".into()));
        };
        let mut adapter =
            check_compatible_with_hyper_wrap_stream(MessageStreamAdapter::<
                TestMessage,
                TestServiceError,
            >::new(
                Marshaller, TestSigner, Box::pin(stream)
            ));

        let mut sent_bytes = adapter.next().await.unwrap().unwrap();
        let sent = Message::read_from(&mut sent_bytes).unwrap();
        assert_eq!("signed", sent.headers()[0].name().as_str());
        assert_eq!(&HeaderValue::Bool(true), sent.headers()[0].value());
        let inner = Message::read_from(&mut (&sent.payload()[..])).unwrap();
        assert_eq!(&b"test"[..], &inner.payload()[..]);

        let mut end_signal_bytes = adapter.next().await.unwrap().unwrap();
        let end_signal = Message::read_from(&mut end_signal_bytes).unwrap();
        assert_eq!("signed", end_signal.headers()[0].name().as_str());
        assert_eq!(&HeaderValue::Bool(true), end_signal.headers()[0].value());
        assert_eq!(0, end_signal.payload().len());
    }

    #[tokio::test]
    async fn message_stream_adapter_construction_failure() {
        let stream = stream! {
            yield Err(EventStreamError::InvalidMessageLength.into());
        };
        let mut adapter =
            check_compatible_with_hyper_wrap_stream(MessageStreamAdapter::<
                TestMessage,
                TestServiceError,
            >::new(
                Marshaller, TestSigner, Box::pin(stream)
            ));

        let result = adapter.next().await.unwrap();
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            SdkError::ConstructionFailure(_)
        ));
    }

    // Verify the developer experience for this compiles
    #[allow(unused)]
    fn event_stream_input_ergonomics() {
        fn check(input: impl Into<EventStreamInput<TestMessage>>) {
            let _: EventStreamInput<TestMessage> = input.into();
        }
        check(stream! {
            yield Ok(TestMessage("test".into()));
        });
        check(stream! {
            yield Err(EventStreamError::InvalidMessageLength.into());
        });
    }
}
