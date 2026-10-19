/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Server-only framing for a CBOR document containing a live blob.

use super::rpc_v2_cbor::RpcV2CborSerde;
use crate::body::BoxBody;
use aws_smithy_cbor::Encoder;
use aws_smithy_schema::codec::{Codec, FinishSerializer};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::{BigDecimal, BigInteger, Blob, DateTime, Document};
use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use std::pin::Pin;
use std::task::{Context, Poll};

pub(super) fn body(
    codec: &RpcV2CborSerde,
    output: &Schema<'_>,
    value: &dyn SerializableStruct,
    member: &Schema<'_>,
    source: BoxBody,
) -> Result<BoxBody, SerdeError> {
    let mut serializer = OuterSerializer(MemberSerializer {
        codec,
        bytes: Vec::new(),
    });
    serializer.write_struct(output, value)?;
    let mut encoder = Encoder::new(serializer.0.bytes);
    encoder.str(member_name(member)?);
    let mut prefix = encoder.into_writer();
    // The public encoder has no indefinite-byte-string opener.
    prefix.push(0x5f);
    Ok(crate::body::boxed(StreamingBody {
        source,
        state: State::Prefix(prefix.into()),
    }))
}

fn member_name<'a>(schema: &'a Schema<'_>) -> Result<&'a str, SerdeError> {
    schema
        .member_name()
        .ok_or_else(|| SerdeError::custom("CBOR streaming response member has no name"))
}

struct MemberSerializer<'a> {
    codec: &'a RpcV2CborSerde,
    bytes: Vec<u8>,
}

impl MemberSerializer<'_> {
    fn member(
        &mut self,
        schema: &Schema<'_>,
        write: impl FnOnce(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        let mut encoder = Encoder::new(std::mem::take(&mut self.bytes));
        encoder.str(member_name(schema)?);
        self.bytes = encoder.into_writer();
        let mut serializer = self.codec.create_serializer();
        write(&mut serializer)?;
        self.bytes.extend(serializer.finish());
        Ok(())
    }
}

struct OuterSerializer<'a>(MemberSerializer<'a>);

impl OuterSerializer<'_> {
    fn member(
        &mut self,
        _: &Schema<'_>,
        _: impl FnOnce(&mut dyn ShapeSerializer) -> Result<(), SerdeError>,
    ) -> Result<(), SerdeError> {
        Err(SerdeError::custom(
            "CBOR streaming response requires an outer structure",
        ))
    }
}

// Delegate the complete value at the codec root. Nested callbacks therefore receive
// the normal codec serializer, never the outer-map interception.
macro_rules! member_methods {
    ($($method:ident($($arg:ident: $ty:ty),*);)*) => {$ (
        fn $method(&mut self, schema: &Schema<'_>, $($arg: $ty),*) -> Result<(), SerdeError> {
            self.member(schema, |serializer| serializer.$method(schema, $($arg),*))
        }
    )*};
}

macro_rules! ordinary_methods {
    () => {
        member_methods! {
            write_boolean(value: bool);
            write_byte(value: i8);
            write_short(value: i16);
            write_integer(value: i32);
            write_long(value: i64);
            write_float(value: f32);
            write_double(value: f64);
            write_big_integer(value: &BigInteger);
            write_big_decimal(value: &BigDecimal);
            write_string(value: &str);
            write_blob(value: Blob);
            write_timestamp(value: &DateTime);
            write_document(value: &Document);
            write_null();
            write_list(write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>);
            write_map(write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>);
            write_list_with_length(length: usize, write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>);
            write_map_with_length(length: usize, write: &dyn Fn(&mut dyn ShapeSerializer) -> Result<(), SerdeError>);
            write_string_list(values: &[String]);
            write_blob_list(values: &[Blob]);
            write_integer_list(values: &[i32]);
            write_long_list(values: &[i64]);
            write_string_string_map(values: &std::collections::HashMap<String, String>);
        }
    };
}

impl ShapeSerializer for MemberSerializer<'_> {
    member_methods! { write_struct(value: &dyn SerializableStruct); }
    ordinary_methods!();
}

impl ShapeSerializer for OuterSerializer<'_> {
    fn write_struct(&mut self, _: &Schema<'_>, value: &dyn SerializableStruct) -> Result<(), SerdeError> {
        let mut encoder = Encoder::new(Vec::new());
        encoder.begin_map();
        self.0.bytes = encoder.into_writer();
        value.serialize_members(&mut self.0)
    }
    ordinary_methods!();
}

enum State {
    Prefix(Bytes),
    Streaming,
    Suffix(Option<http::HeaderMap>),
    Trailers(http::HeaderMap),
    Done,
}

struct StreamingBody {
    source: BoxBody,
    state: State,
}

impl Body for StreamingBody {
    type Data = Bytes;
    type Error = crate::Error;

    fn poll_frame(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let this = self.get_mut();
        match &mut this.state {
            State::Prefix(prefix) => {
                let prefix = std::mem::take(prefix);
                this.state = State::Streaming;
                Poll::Ready(Some(Ok(Frame::data(prefix))))
            }
            State::Streaming => match Pin::new(&mut this.source).poll_frame(cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Some(Err(err))) => {
                    this.state = State::Done;
                    Poll::Ready(Some(Err(err)))
                }
                Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                    Ok(data) if data.is_empty() => {
                        // Yield after each empty chunk so a ready source cannot monopolize a task.
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    Ok(data) => {
                        let mut encoder = Encoder::new(Vec::new());
                        encoder.blob_bytes(&data);
                        Poll::Ready(Some(Ok(Frame::data(encoder.into_writer().into()))))
                    }
                    Err(frame) => {
                        // Trailers terminate the source, but the CBOR document must close first.
                        this.state = State::Suffix(frame.into_trailers().ok());
                        Pin::new(this).poll_frame(cx)
                    }
                },
                Poll::Ready(None) => {
                    this.state = State::Suffix(None);
                    Pin::new(this).poll_frame(cx)
                }
            },
            State::Suffix(trailers) => {
                this.state = trailers.take().map_or(State::Done, State::Trailers);
                Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(&[0xff, 0xff])))))
            }
            State::Trailers(trailers) => {
                let trailers = std::mem::take(trailers);
                this.state = State::Done;
                Poll::Ready(Some(Ok(Frame::trailers(trailers))))
            }
            State::Done => Poll::Ready(None),
        }
    }

    fn is_end_stream(&self) -> bool {
        matches!(self.state, State::Done)
    }
    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests {
    use super::super::{RpcV2Cbor, ServerProtocol};
    use super::*;
    use aws_smithy_schema::{prelude, shape_id, ShapeType};
    use http_body_util::BodyExt;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    static DATA: Schema =
        Schema::new_member(shape_id!("test", "Output", "fahad"), ShapeType::Blob, "fahad", 0).with_streaming();
    static OUTPUT: Schema = Schema::new_struct(shape_id!("test", "Output"), ShapeType::Structure, &[&DATA]);
    struct Empty;
    impl SerializableStruct for Empty {
        fn schema(&self) -> &Schema<'_> {
            &OUTPUT
        }
        fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
            Ok(())
        }
    }
    fn response(source: BoxBody) -> crate::response::Response {
        RpcV2Cbor::default().serialize_streaming_response(&OUTPUT, &Empty, &DATA, Some(source))
    }
    fn chunks(values: Vec<Result<Bytes, std::io::Error>>) -> BoxBody {
        crate::body::wrap_stream(futures_util::stream::iter(values))
    }
    fn ok(value: &'static [u8]) -> Result<Bytes, std::io::Error> {
        Ok(Bytes::from_static(value))
    }
    async fn data(body: &mut BoxBody) -> Bytes {
        body.frame().await.unwrap().unwrap().into_data().unwrap()
    }

    #[tokio::test]
    async fn empty_single_and_multiple_chunks_preserve_schema_name_and_framing() {
        for (input, expected) in [
            (vec![], b"\xbf\x65fahad\x5f\xff\xff".as_slice()),
            (vec![ok(b"hello")], b"\xbf\x65fahad\x5f\x45hello\xff\xff"),
            (
                vec![ok(b"he"), ok(b""), ok(b"llo")],
                b"\xbf\x65fahad\x5f\x42he\x43llo\xff\xff",
            ),
        ] {
            let response = response(chunks(input));
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["content-type"], "application/cbor");
            assert_eq!(response.headers()["smithy-protocol"], "rpc-v2-cbor");
            assert!(!response.headers().contains_key("content-length"));
            assert_eq!(response.body().size_hint().exact(), None);
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes().as_ref(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn trailers_follow_the_document_suffix_and_end_the_stream() {
        use futures_util::StreamExt;

        let mut trailers = http::HeaderMap::new();
        trailers.append("x-checksum", "first".parse().unwrap());
        trailers.append("x-checksum", "second".parse().unwrap());
        trailers.insert("x-stream-status", "complete".parse().unwrap());
        for chunks in [vec![], vec![b"hello".as_slice()], vec![b"he".as_slice(), b"", b"llo"]] {
            let frames = chunks
                .iter()
                .map(|chunk| Frame::data(Bytes::copy_from_slice(chunk)))
                .chain(std::iter::once(Frame::trailers(trailers.clone())))
                .map(Ok::<_, std::io::Error>)
                .collect::<Vec<_>>();
            // Trailers terminate the source; closing the document must not require another poll.
            let source = http_body_util::StreamBody::new(
                futures_util::stream::iter(frames).chain(futures_util::stream::pending()),
            );
            let mut body = response(crate::body::boxed(source)).into_body();
            assert_eq!(data(&mut body).await.as_ref(), b"\xbf\x65fahad\x5f");
            for chunk in chunks.iter().filter(|chunk| !chunk.is_empty()) {
                let mut encoder = Encoder::new(Vec::new());
                encoder.blob_bytes(chunk);
                assert_eq!(data(&mut body).await.as_ref(), encoder.into_writer());
            }
            assert_eq!(data(&mut body).await.as_ref(), &[0xff, 0xff]);
            assert!(!body.is_end_stream(), "trailers are still pending");
            assert_eq!(body.frame().await.unwrap().unwrap().into_trailers().unwrap(), trailers);
            assert!(body.is_end_stream());
            assert!(body.frame().await.is_none());
        }
    }

    #[tokio::test]
    async fn absent_and_present_empty_are_distinct() {
        let protocol = RpcV2Cbor::default();
        let absent = protocol
            .serialize_streaming_response(&OUTPUT, &Empty, &DATA, None)
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        let present = response(crate::body::empty())
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(absent.as_ref(), &[0xbf, 0xff]);
        assert_eq!(present.as_ref(), b"\xbf\x65fahad\x5f\xff\xff");
    }

    #[tokio::test]
    async fn prefix_does_not_poll_and_pending_does_not_close() {
        let polls = Arc::new(AtomicUsize::new(0));
        let counter = polls.clone();
        let source = crate::body::wrap_stream(futures_util::stream::poll_fn(move |_| {
            counter.fetch_add(1, Ordering::SeqCst);
            Poll::<Option<Result<Bytes, std::io::Error>>>::Pending
        }));
        let mut body = response(source).into_body();
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        assert_eq!(data(&mut body).await.as_ref(), b"\xbf\x65fahad\x5f");
        assert_eq!(polls.load(Ordering::SeqCst), 0);
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        assert!(!body.is_end_stream());
    }

    #[tokio::test]
    async fn delayed_chunks_and_suffix_wait_for_eof() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let source = crate::body::wrap_stream(futures_util::stream::once(async move {
            receiver.await.unwrap();
            ok(b"hello")
        }));
        let mut body = response(source).into_body();
        assert_eq!(data(&mut body).await.as_ref(), b"\xbf\x65fahad\x5f");
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        sender.send(()).unwrap();
        assert_eq!(data(&mut body).await.as_ref(), b"\x45hello");
        assert_eq!(data(&mut body).await.as_ref(), &[0xff, 0xff]);
        assert!(body.frame().await.is_none());
    }

    #[tokio::test]
    async fn errors_before_and_after_data_never_close_the_document() {
        for partial in [false, true] {
            let mut input = vec![];
            if partial {
                input.push(ok(b"he"));
            }
            input.push(Err(std::io::Error::other("stream failed")));
            let mut body = response(chunks(input)).into_body();
            assert_eq!(data(&mut body).await.as_ref(), b"\xbf\x65fahad\x5f");
            if partial {
                assert_eq!(data(&mut body).await.as_ref(), b"\x42he");
            }
            assert!(body.frame().await.unwrap().is_err());
            assert!(body.is_end_stream());
            assert!(body.frame().await.is_none());
        }
    }

    #[test]
    fn empty_chunks_yield_after_one_poll_and_drop_cancels_source() {
        struct Source(Arc<AtomicUsize>, Arc<AtomicUsize>);
        impl Drop for Source {
            fn drop(&mut self) {
                self.1.fetch_add(1, Ordering::SeqCst);
            }
        }
        impl Body for Source {
            type Data = Bytes;
            type Error = std::io::Error;
            fn poll_frame(
                self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Poll::Ready(Some(Ok(Frame::data(Bytes::new()))))
            }
        }
        let polls = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let mut body = response(crate::body::boxed(Source(polls.clone(), drops.clone()))).into_body();
        let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_ready());
        assert!(Pin::new(&mut body).poll_frame(&mut cx).is_pending());
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        drop(body);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn ordinary_failure_and_missing_name_fail_before_success_response() {
        struct Fail;
        impl SerializableStruct for Fail {
            fn schema(&self) -> &Schema<'_> {
                &OUTPUT
            }
            fn serialize_members(&self, _: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                Err(SerdeError::custom("ordinary member failed"))
            }
        }
        let protocol = RpcV2Cbor::default();
        assert_eq!(
            protocol
                .serialize_streaming_response(&OUTPUT, &Fail, &DATA, Some(crate::body::empty()))
                .status(),
            400
        );
        assert_eq!(
            protocol
                .serialize_streaming_response(&OUTPUT, &Empty, &prelude::BLOB, Some(crate::body::empty()))
                .status(),
            400
        );
    }

    #[test]
    fn codec_errors_are_preserved_and_non_structure_roots_are_rejected() {
        let codec = RpcV2CborSerde::default();
        let mut normal = codec.create_serializer();
        let mut member = MemberSerializer {
            codec: &codec,
            bytes: Vec::new(),
        };
        for kind in 0..3 {
            let write = |s: &mut dyn ShapeSerializer| match kind {
                0 => s.write_document(&DATA, &Document::Null),
                1 => s.write_big_integer(&DATA, &"123".parse::<BigInteger>().unwrap()),
                _ => s.write_big_decimal(&DATA, &"1.5".parse::<BigDecimal>().unwrap()),
            };
            assert_eq!(format!("{:?}", write(&mut normal)), format!("{:?}", write(&mut member)));
        }
        let mut outer = OuterSerializer(member);
        assert!(outer.write_null(&OUTPUT).is_err());
        assert!(outer.write_string(&OUTPUT, "invalid root").is_err());
    }

    #[tokio::test]
    async fn every_ordinary_write_delegates_to_the_configured_codec() {
        static MEMBER: Schema = Schema::new_member(shape_id!("test", "Output", "value"), ShapeType::String, "value", 1);
        struct Nested;
        impl SerializableStruct for Nested {
            fn schema(&self) -> &Schema<'_> {
                &OUTPUT
            }
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                s.write_string(&MEMBER, "nested value")
            }
        }
        struct Values;
        impl SerializableStruct for Values {
            fn schema(&self) -> &Schema<'_> {
                &OUTPUT
            }
            fn serialize_members(&self, s: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
                // Repeated keys are intentional here: compare each trait entry point with
                // the existing codec, including its definite collection-length setting.
                s.write_boolean(&MEMBER, true)?;
                s.write_byte(&MEMBER, -1)?;
                s.write_short(&MEMBER, -2)?;
                s.write_integer(&MEMBER, -3)?;
                s.write_long(&MEMBER, -4)?;
                s.write_float(&MEMBER, 1.5)?;
                s.write_double(&MEMBER, 2.5)?;
                s.write_string(&MEMBER, "hi")?;
                s.write_blob(&MEMBER, Blob::new(b"blob"))?;
                s.write_timestamp(&MEMBER, &DateTime::from_secs(123))?;
                s.write_null(&MEMBER)?;
                s.write_struct(&MEMBER, &Nested)?;
                let elements = |s: &mut dyn ShapeSerializer| {
                    s.write_struct(&prelude::STRING, &Nested)?;
                    s.write_null(&prelude::STRING)
                };
                s.write_list(&MEMBER, &elements)?;
                s.write_list_with_length(&MEMBER, 2, &elements)?;
                let entries = |s: &mut dyn ShapeSerializer| {
                    s.write_string(&prelude::STRING, "nested")?;
                    s.write_list_with_length(&MEMBER, 2, &elements)
                };
                s.write_map(&MEMBER, &entries)?;
                s.write_map_with_length(&MEMBER, 1, &entries)?;
                s.write_string_list(&MEMBER, &["a".into()])?;
                s.write_blob_list(&MEMBER, &[Blob::new(b"b")])?;
                s.write_integer_list(&MEMBER, &[1, 2])?;
                s.write_long_list(&MEMBER, &[3, 4])?;
                s.write_string_string_map(&MEMBER, &[("k".into(), "v".into())].into())
            }
        }
        let codec = RpcV2CborSerde::default();
        let mut normal = codec.create_serializer();
        normal.write_struct(&OUTPUT, &Values).unwrap();
        let mut expected = normal.finish();
        // The reference codec closes its outer map; the streaming prefix must leave it open.
        assert_eq!(expected.pop(), Some(0xff));
        expected.extend_from_slice(b"\x65fahad\x5f\x42he\xff\xff");
        let actual = body(&codec, &OUTPUT, &Values, &DATA, chunks(vec![ok(b"he")]))
            .unwrap()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(actual.as_ref(), expected);
    }
}
