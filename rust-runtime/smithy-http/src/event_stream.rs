/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Provides Sender/Receiver implementations for Event Stream codegen.

use crate::body::SdkBody;
use crate::result::SdkError;
use bytes::Bytes;
use bytes_utils::SegmentedBuf;
use hyper::body::HttpBody;
use smithy_eventstream::frame::{
    DecodedFrame, MarshallMessage, MessageFrameDecoder, SignMessage, UnmarshallMessage,
};
use std::error::Error as StdError;
use std::marker::PhantomData;
use std::ops::DerefMut;

enum SenderState {
    /// A Hyper body channel has been created for the Event Stream input,
    /// but it's not hooked up to anything yet.
    NotReady(hyper::Body),
    /// The Hyper body channel is all wired up, and we have a message signer implementation.
    Ready(Box<dyn SignMessage>),
}

/// Sends Smithy-modeled messages on an Event Stream.
pub struct Sender<T, E: StdError + Send + Sync> {
    marshaller: Box<dyn MarshallMessage<Input = T>>,
    http_sender: hyper::body::Sender,
    state: SenderState,
    _phantom: PhantomData<E>,
}

impl<T, E: StdError + Send + Sync> Sender<T, E> {
    /// Creates a new `Sender` with the given message marshaller.
    pub fn new(marshaller: impl MarshallMessage<Input = T> + 'static) -> Self {
        let (http_sender, http_body) = hyper::Body::channel();
        Sender {
            marshaller: Box::new(marshaller),
            http_sender,
            state: SenderState::NotReady(http_body),
            _phantom: Default::default(),
        }
    }

    fn signer_mut(&mut self) -> &mut dyn SignMessage {
        match &mut self.state {
            SenderState::NotReady(_) => panic!("Sender is not ready to send data yet"),
            SenderState::Ready(ref mut state) => state.deref_mut(),
        }
    }

    /// Asynchronously sends a message on the stream. Returns `Ok(())` if the message was sent
    /// successfully. Returns `Err(SdkError::ConstructionFailure)` if the message couldn't be
    /// marshalled into an Event Stream frame, (e.g., if the message payload was too large).
    /// Returns `Err(SdkError::DispatchFailure)` if a transport failure occurred.
    pub async fn send(&mut self, input: T) -> Result<(), SdkError<E>> {
        let message = self
            .marshaller
            .marshall(input)
            .map_err(|err| SdkError::ConstructionFailure(Box::new(err)))?;
        let message = self
            .signer_mut()
            .sign(message)
            .map_err(|err| SdkError::ConstructionFailure(err))?;
        let mut buffer = Vec::new();
        message
            .write_to(&mut buffer)
            .map_err(|err| SdkError::ConstructionFailure(Box::new(err)))?;
        self.http_sender
            .send_data(buffer.into())
            .await
            .map_err(|err| SdkError::DispatchFailure(Box::new(err)))
    }

    // Intended to be used by the generated code to connect the `Sender` to the actual request.
    #[doc(hidden)]
    pub fn make_ready(&mut self, signer: impl SignMessage + 'static) -> hyper::Body {
        let new_state = SenderState::Ready(Box::new(signer));
        match std::mem::replace(&mut self.state, new_state) {
            SenderState::Ready(_) => panic!("`make_ready` can only be called once"),
            SenderState::NotReady(body) => body,
        }
    }
}

/// Receives Smithy-modeled messages out of an Event Stream.
pub struct Receiver<T, E: StdError + Send + Sync> {
    unmarshaller: Box<dyn UnmarshallMessage<Output = T>>,
    decoder: MessageFrameDecoder,
    buffer: SegmentedBuf<Bytes>,
    body: SdkBody,
    _phantom: PhantomData<E>,
}

impl<T, E: StdError + Send + Sync> Receiver<T, E> {
    /// Creates a new `Receiver` with the given message unmarshaller and SDK body.
    pub fn new(unmarshaller: impl UnmarshallMessage<Output = T> + 'static, body: SdkBody) -> Self {
        Receiver {
            unmarshaller: Box::new(unmarshaller),
            decoder: MessageFrameDecoder::new(),
            buffer: SegmentedBuf::new(),
            body,
            _phantom: Default::default(),
        }
    }

    /// Asynchronously tries to receive a message from the stream. If the stream has ended,
    /// it returns an `Ok(None)`. If there is a transport layer error, it will return
    /// `Err(SdkError::DispatchFailure)`. Service-modeled errors will be a part of the returned
    /// messages.
    pub async fn recv(&mut self) -> Result<Option<T>, SdkError<E>> {
        let next_chunk = self
            .body
            .data()
            .await
            .transpose()
            .map_err(|err| SdkError::DispatchFailure(err))?;
        if let Some(chunk) = next_chunk {
            // The SegmentedBuf will automatically purge when it reads off the end of a chunk boundary
            self.buffer.push(chunk);
            if let DecodedFrame::Complete(message) = self
                .decoder
                .decode_frame(&mut self.buffer)
                .map_err(|err| SdkError::DispatchFailure(Box::new(err)))?
            {
                return Ok(Some(
                    self.unmarshaller
                        .unmarshall(message)
                        .map_err(|err| SdkError::DispatchFailure(Box::new(err)))?,
                ));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{MarshallMessage, Receiver, Sender, UnmarshallMessage};
    use crate::body::SdkBody;
    use crate::result::SdkError;
    use bytes::Bytes;
    use hyper::body::{Body, HttpBody};
    use smithy_eventstream::error::Error as EventStreamError;
    use smithy_eventstream::frame::{Header, HeaderValue, Message, SignMessage, SignMessageError};
    use std::error::Error as StdError;
    use std::io::{Error as IOError, ErrorKind};

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
    struct UnmarshalledMessage(String);

    struct Marshaller;
    impl MarshallMessage for Marshaller {
        type Input = UnmarshalledMessage;

        fn marshall(&self, input: Self::Input) -> Result<Message, EventStreamError> {
            Ok(Message::new(input.0.as_bytes().to_vec()))
        }
    }

    struct Unmarshaller;
    impl UnmarshallMessage for Unmarshaller {
        type Output = UnmarshalledMessage;

        fn unmarshall(&self, message: Message) -> Result<Self::Output, EventStreamError> {
            Ok(UnmarshalledMessage(
                std::str::from_utf8(&message.payload()[..]).unwrap().into(),
            ))
        }
    }

    #[tokio::test]
    async fn receive_success() {
        let chunks: Vec<Result<_, IOError>> =
            vec![Ok(encode_message("one")), Ok(encode_message("two"))];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver =
            Receiver::<UnmarshalledMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            UnmarshalledMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert_eq!(
            UnmarshalledMessage("two".into()),
            receiver.recv().await.unwrap().unwrap()
        );
    }

    #[tokio::test]
    async fn receive_network_failure() {
        let chunks: Vec<Result<_, IOError>> = vec![
            Ok(encode_message("one")),
            Err(IOError::new(ErrorKind::ConnectionReset, FakeError)),
        ];
        let chunk_stream = futures_util::stream::iter(chunks);
        let body = SdkBody::from(Body::wrap_stream(chunk_stream));
        let mut receiver =
            Receiver::<UnmarshalledMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            UnmarshalledMessage("one".into()),
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
        let mut receiver =
            Receiver::<UnmarshalledMessage, EventStreamError>::new(Unmarshaller, body);
        assert_eq!(
            UnmarshalledMessage("one".into()),
            receiver.recv().await.unwrap().unwrap()
        );
        assert!(matches!(
            receiver.recv().await,
            Err(SdkError::DispatchFailure(_))
        ));
    }

    struct TestSigner;
    impl SignMessage for TestSigner {
        fn sign(&self, message: Message) -> Result<Message, SignMessageError> {
            let mut buffer = Vec::new();
            message.write_to(&mut buffer).unwrap();
            Ok(Message::new(buffer).add_header(Header::new("signed", HeaderValue::Bool(true))))
        }
    }

    #[tokio::test]
    async fn send_success() {
        let mut sender = Sender::<UnmarshalledMessage, EventStreamError>::new(Marshaller);
        let mut http_body = sender.make_ready(TestSigner);

        sender
            .send(UnmarshalledMessage("test".into()))
            .await
            .unwrap();

        let mut sent_bytes = http_body.data().await.unwrap().unwrap();
        let sent = Message::read_from(&mut sent_bytes).unwrap();
        assert_eq!("signed", sent.headers()[0].name().as_str());
        assert_eq!(&HeaderValue::Bool(true), sent.headers()[0].value());
        let inner = Message::read_from(&mut (&sent.payload()[..])).unwrap();
        assert_eq!(&b"test"[..], &inner.payload()[..]);
    }

    #[tokio::test]
    async fn send_network_failure() {
        let mut sender = Sender::<UnmarshalledMessage, EventStreamError>::new(Marshaller);
        let http_body = sender.make_ready(TestSigner);
        drop(http_body);

        let result = sender.send(UnmarshalledMessage("test".into())).await;
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            SdkError::DispatchFailure(_)
        ));
    }

    #[tokio::test]
    async fn send_construction_failure() {
        struct BadMarshaller;
        impl MarshallMessage for BadMarshaller {
            type Input = UnmarshalledMessage;

            fn marshall(&self, _input: Self::Input) -> Result<Message, EventStreamError> {
                Err(EventStreamError::InvalidMessageLength)
            }
        }

        let mut sender = Sender::<UnmarshalledMessage, EventStreamError>::new(BadMarshaller);
        let _http_body = sender.make_ready(TestSigner);

        let result = sender.send(UnmarshalledMessage("test".into())).await;
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            SdkError::ConstructionFailure(_)
        ));
    }
}
