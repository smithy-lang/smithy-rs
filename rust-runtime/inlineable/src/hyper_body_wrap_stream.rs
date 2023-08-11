/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_http::body::SdkBody;
use aws_smithy_http::byte_stream::error::Error as ByteStreamError;
use aws_smithy_http::byte_stream::ByteStream;
use aws_smithy_http::event_stream::MessageStreamAdapter;
use aws_smithy_http::result::SdkError;
use bytes::Bytes;
use futures_core::stream::Stream;
use std::error::Error as StdError;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(crate) struct HyperBodyWrapEventStream<T, E>(MessageStreamAdapter<T, E>);

impl<T, E> HyperBodyWrapEventStream<T, E> {
    #[allow(dead_code)]
    pub(crate) fn new(adapter: MessageStreamAdapter<T, E>) -> Self {
        Self(adapter)
    }
}

impl<T, E: StdError + Send + Sync + 'static> Unpin for HyperBodyWrapEventStream<T, E> {}

impl<T, E: StdError + Send + Sync + 'static> Stream for HyperBodyWrapEventStream<T, E> {
    type Item = Result<Bytes, SdkError<E>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}

pub(crate) struct HyperBodyWrapByteStream(ByteStream);

impl HyperBodyWrapByteStream {
    #[allow(dead_code)]
    pub(crate) fn new(stream: ByteStream) -> Self {
        Self(stream)
    }

    #[allow(dead_code)]
    pub(crate) fn into_inner(self) -> SdkBody {
        self.0.into_inner()
    }
}

impl Unpin for HyperBodyWrapByteStream {}

impl Stream for HyperBodyWrapByteStream {
    type Item = Result<Bytes, ByteStreamError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_async::future::fn_stream::FnStream;
    use aws_smithy_eventstream::error::Error;
    use aws_smithy_eventstream::frame::MarshallMessage;
    use aws_smithy_eventstream::frame::{Message, NoOpSigner};
    use futures_core::stream::Stream;

    #[derive(Debug, Eq, PartialEq)]
    struct TestMessage(String);

    #[derive(Debug)]
    struct Marshaller;
    impl MarshallMessage for Marshaller {
        type Input = TestMessage;

        fn marshall(&self, input: Self::Input) -> Result<Message, Error> {
            Ok(Message::new(input.0.as_bytes().to_vec()))
        }
    }
    #[derive(Debug)]
    struct ErrorMarshaller;
    impl MarshallMessage for ErrorMarshaller {
        type Input = TestServiceError;

        fn marshall(&self, _input: Self::Input) -> Result<Message, Error> {
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

    fn check_compatible_with_hyper_wrap_stream<S, O, E>(stream: S) -> S
    where
        S: Stream<Item = Result<O, E>> + Send + 'static,
        O: Into<Bytes> + 'static,
        E: Into<Box<dyn StdError + Send + Sync + 'static>> + 'static,
    {
        stream
    }

    #[test]
    fn test_message_adapter_stream_can_be_made_compatible_with_hyper_wrap_stream() {
        let stream = FnStream::new(|tx| {
            Box::pin(async move {
                let message = Ok(TestMessage("test".into()));
                tx.send(message).await.expect("failed to send");
            })
        });
        check_compatible_with_hyper_wrap_stream(HyperBodyWrapEventStream(MessageStreamAdapter::<
            TestMessage,
            TestServiceError,
        >::new(
            Marshaller,
            ErrorMarshaller,
            NoOpSigner {},
            stream,
        )));
    }

    #[test]
    fn test_byte_stream_stream_can_be_made_compatible_with_hyper_wrap_stream() {
        let stream = ByteStream::from_static(b"Hello world");
        check_compatible_with_hyper_wrap_stream(HyperBodyWrapByteStream::new(stream));
    }
}
