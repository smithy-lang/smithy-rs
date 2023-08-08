/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::result::SdkError;
use aws_smithy_async::future::fn_stream::FnStream;
use aws_smithy_eventstream::frame::{MarshallMessage, SignMessage};
use bytes::Bytes;
use std::error::Error as StdError;
use std::fmt;
use std::fmt::Debug;
use std::future::poll_fn;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use tracing::trace;

/// Input type for Event Streams.
pub struct EventStreamSender<T, E> {
    // `FnStream` does not have a `Sync` bound but this struct needs to be `Sync`
    // as demonstrated by a unit test `event_stream_sender_send`.
    // Wrapping `input_stream` with a `Mutex` will make `EventStreamSender` `Sync`.
    input_stream: Mutex<FnStream<Result<T, E>>>,
}

impl<T, E> Debug for EventStreamSender<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name_t = std::any::type_name::<T>();
        let name_e = std::any::type_name::<E>();
        write!(f, "EventStreamSender<{name_t}, {name_e}>")
    }
}

impl<T, E: StdError + Send + Sync + 'static> EventStreamSender<T, E> {
    #[doc(hidden)]
    pub fn into_body_stream(
        self,
        marshaller: impl MarshallMessage<Input = T> + Send + Sync + 'static,
        error_marshaller: impl MarshallMessage<Input = E> + Send + Sync + 'static,
        signer: impl SignMessage + Send + Sync + 'static,
    ) -> MessageStreamAdapter<T, E> {
        MessageStreamAdapter::new(
            marshaller,
            error_marshaller,
            signer,
            std::mem::replace(&mut *self.input_stream.lock().unwrap(), FnStream::taken()),
        )
    }
}

impl<T, E> From<FnStream<Result<T, E>>> for EventStreamSender<T, E> {
    fn from(stream: FnStream<Result<T, E>>) -> Self {
        EventStreamSender {
            input_stream: Mutex::new(stream),
        }
    }
}

/// An error that occurs within a message stream.
#[derive(Debug)]
pub struct MessageStreamError {
    kind: MessageStreamErrorKind,
    pub(crate) meta: aws_smithy_types::Error,
}

#[derive(Debug)]
enum MessageStreamErrorKind {
    Unhandled(Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl MessageStreamError {
    /// Creates the `MessageStreamError::Unhandled` variant from any error type.
    pub fn unhandled(err: impl Into<Box<dyn std::error::Error + Send + Sync + 'static>>) -> Self {
        Self {
            meta: Default::default(),
            kind: MessageStreamErrorKind::Unhandled(err.into()),
        }
    }

    /// Creates the `MessageStreamError::Unhandled` variant from a `aws_smithy_types::Error`.
    pub fn generic(err: aws_smithy_types::Error) -> Self {
        Self {
            meta: err.clone(),
            kind: MessageStreamErrorKind::Unhandled(err.into()),
        }
    }

    /// Returns error metadata, which includes the error code, message,
    /// request ID, and potentially additional information.
    pub fn meta(&self) -> &aws_smithy_types::Error {
        &self.meta
    }
}

impl StdError for MessageStreamError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.kind {
            MessageStreamErrorKind::Unhandled(source) => Some(source.as_ref() as _),
        }
    }
}

impl fmt::Display for MessageStreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            MessageStreamErrorKind::Unhandled(_) => write!(f, "message stream error"),
        }
    }
}

/// Adapts a `Stream<SmithyMessageType>` to a signed `Stream<Bytes>` by using the provided
/// message marshaller and signer implementations.
///
/// This will yield an `Err(SdkError::ConstructionFailure)` if a message can't be
/// marshalled into an Event Stream frame, (e.g., if the message payload was too large).
#[allow(missing_debug_implementations)]
pub struct MessageStreamAdapter<T, E> {
    marshaller: Box<dyn MarshallMessage<Input = T> + Send + Sync>,
    error_marshaller: Box<dyn MarshallMessage<Input = E> + Send + Sync>,
    signer: Box<dyn SignMessage + Send + Sync>,
    stream: FnStream<Result<T, E>>,
    end_signal_sent: bool,
    _phantom: PhantomData<E>,
}

impl<T, E: StdError + Send + Sync + 'static> Unpin for MessageStreamAdapter<T, E> {}

impl<T, E> MessageStreamAdapter<T, E> {
    /// Create a new `MessageStreamAdapter`.
    pub fn new(
        marshaller: impl MarshallMessage<Input = T> + Send + Sync + 'static,
        error_marshaller: impl MarshallMessage<Input = E> + Send + Sync + 'static,
        signer: impl SignMessage + Send + Sync + 'static,
        stream: FnStream<Result<T, E>>,
    ) -> Self {
        MessageStreamAdapter {
            marshaller: Box::new(marshaller),
            error_marshaller: Box::new(error_marshaller),
            signer: Box::new(signer),
            stream,
            end_signal_sent: false,
            _phantom: Default::default(),
        }
    }
}

impl<T, E: StdError + Send + Sync + 'static> MessageStreamAdapter<T, E> {
    /// Consumes and returns the next item from this stream.
    pub async fn next(&mut self) -> Option<Result<Bytes, SdkError<E>>> {
        let mut me = Pin::new(self);
        poll_fn(|cx| me.as_mut().poll_next(cx)).await
    }

    /// Attempts to pull out the next value of this stream, returning `None` if the stream is
    /// exhausted.
    pub fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, SdkError<E>>>> {
        match Pin::new(&mut self.stream).as_mut().poll_next(cx) {
            Poll::Ready(message_option) => {
                if let Some(message_result) = message_option {
                    let message = match message_result {
                        Ok(message) => self
                            .marshaller
                            .marshall(message)
                            .map_err(SdkError::construction_failure)?,
                        Err(message) => self
                            .error_marshaller
                            .marshall(message)
                            .map_err(SdkError::construction_failure)?,
                    };

                    trace!(unsigned_message = ?message, "signing event stream message");
                    let message = self
                        .signer
                        .sign(message)
                        .map_err(SdkError::construction_failure)?;

                    let mut buffer = Vec::new();
                    message
                        .write_to(&mut buffer)
                        .map_err(SdkError::construction_failure)?;
                    trace!(signed_message = ?buffer, "sending signed event stream message");
                    Poll::Ready(Some(Ok(Bytes::from(buffer))))
                } else if !self.end_signal_sent {
                    self.end_signal_sent = true;
                    let mut buffer = Vec::new();
                    match self.signer.sign_empty() {
                        Some(sign) => {
                            sign.map_err(SdkError::construction_failure)?
                                .write_to(&mut buffer)
                                .map_err(SdkError::construction_failure)?;
                            trace!(signed_message = ?buffer, "sending signed empty message to terminate the event stream");
                            Poll::Ready(Some(Ok(Bytes::from(buffer))))
                        }
                        None => Poll::Ready(None),
                    }
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::MarshallMessage;
    use crate::event_stream::{EventStreamSender, MessageStreamAdapter};
    use crate::result::SdkError;
    use aws_smithy_async::future::fn_stream::FnStream;
    use aws_smithy_eventstream::error::Error as EventStreamError;
    use aws_smithy_eventstream::frame::{
        Header, HeaderValue, Message, NoOpSigner, SignMessage, SignMessageError,
    };
    use std::error::Error as StdError;

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
    struct ErrorMarshaller;
    impl MarshallMessage for ErrorMarshaller {
        type Input = TestServiceError;

        fn marshall(&self, _input: Self::Input) -> Result<Message, EventStreamError> {
            Err(Message::read_from(&b""[..]).expect_err("this should always fail"))
        }
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

        fn sign_empty(&mut self) -> Option<Result<Message, SignMessageError>> {
            Some(Ok(
                Message::new(&b""[..]).add_header(Header::new("signed", HeaderValue::Bool(true)))
            ))
        }
    }

    fn check_send_sync<T: Send + Sync>(value: T) -> T {
        value
    }

    #[test]
    fn event_stream_sender_send() {
        check_send_sync(EventStreamSender::from(FnStream::new(|tx| {
            Box::pin(async move {
                let message = Result::<_, TestServiceError>::Ok(TestMessage("test".into()));
                tx.send(message).await.expect("failed to send");
            })
        })));
    }

    #[tokio::test]
    async fn message_stream_adapter_success() {
        let stream = FnStream::new(|tx| {
            Box::pin(async move {
                let message = Ok(TestMessage("test".into()));
                tx.send(message).await.expect("failed to send");
            })
        });
        let mut adapter = MessageStreamAdapter::<TestMessage, _>::new(
            Marshaller,
            ErrorMarshaller,
            TestSigner,
            stream,
        );

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
        let stream = FnStream::new(|tx| {
            Box::pin(async move {
                tx.send(Err(TestServiceError))
                    .await
                    .expect("failed to send");
            })
        });
        let mut adapter = MessageStreamAdapter::<TestMessage, TestServiceError>::new(
            Marshaller,
            ErrorMarshaller,
            NoOpSigner {},
            stream,
        );

        let result = adapter.next().await.unwrap();
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            SdkError::ConstructionFailure(_)
        ));
    }
}
