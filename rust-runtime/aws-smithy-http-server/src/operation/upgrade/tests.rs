/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use crate::schema::OperationSchema;
use crate::schema::{HttpModeledError, ServerProtocol, SharedServerProtocol};
use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::{shape_id, Schema, ShapeType};
use bytes::Bytes;
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A perfectly usable HTTP protocol that deliberately has no event-stream capability.
#[derive(Debug, Default)]
struct HttpOnly {
    expected_extension: Option<std::sync::Weak<String>>,
    collect: bool,
}

impl ServerProtocol for HttpOnly {
    fn protocol_id(&self) -> &'static aws_smithy_schema::ShapeId<'static> {
        static ID: aws_smithy_schema::ShapeId<'static> = shape_id!("test", "httpOnly");
        &ID
    }
    fn request_body_requirement(&self, _: &crate::schema::OperationSchema<'_>) -> BodyDirective {
        if self.collect {
            BodyDirective::Collect
        } else {
            BodyDirective::Skip
        }
    }
    fn deserialize_request<'a>(
        &'a self,
        _: &Schema<'_>,
        request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        if let Some(extension) = &self.expected_extension {
            assert_eq!(extension.upgrade().as_deref().map(String::as_str), Some("preserved"));
            assert_eq!(request.method(), "PATCH");
            assert_eq!(request.uri(), "/metadata?key=value");
            assert_eq!(request.headers().get("x-test"), Some("header"));
            assert_eq!(
                request.body().as_ref(),
                if self.collect { b"payload".as_slice() } else { b"" }
            );
        }
        Ok(Box::new(crate::schema::request_bindings::EmptyStructDeserializer))
    }
    fn serialize_response(&self, _: &Schema<'_>, _: &dyn SerializableStruct) -> http::Response<BoxBody> {
        http::Response::new(crate::body::empty())
    }
    fn serialize_streaming_response(
        &self,
        _: &Schema<'_>,
        _: &dyn SerializableStruct,
        body: BoxBody,
    ) -> http::Response<BoxBody> {
        http::Response::new(body)
    }
    fn serialize_error(&self, _: &dyn HttpModeledError) -> http::Response<BoxBody> {
        panic!("unexpected error")
    }
    fn serialize_rejection(&self, err: DeserializeError) -> http::Response<BoxBody> {
        panic!("unexpected rejection: {err}")
    }
}

static EMPTY: Schema<'static> = Schema::new_struct(shape_id!("test", "Empty"), ShapeType::Structure, &[]);
static EVENT: Schema<'static> =
    Schema::new_member(shape_id!("test", "Events", "events"), ShapeType::Union, "events", 0).with_streaming();
static EVENTS: Schema<'static> = Schema::new_struct(shape_id!("test", "Events"), ShapeType::Structure, &[&EVENT]);
static BLOB: Schema<'static> =
    Schema::new_member(shape_id!("test", "Blob", "blob"), ShapeType::Blob, "blob", 0).with_streaming();
static BLOBS: Schema<'static> = Schema::new_struct(shape_id!("test", "Blob"), ShapeType::Structure, &[&BLOB]);

macro_rules! operation {
    ($name:ident, $input:ident, $output:ident) => {
        struct $name;
        impl OperationShape for $name {
            const ID: crate::shape_id::ShapeId = crate::shape_id::ShapeId::new("test#Operation", "test", "Operation");
            type Input = ();
            type Output = ();
            type Error = Infallible;
        }
        impl SchemaOperationShape for $name {
            const SCHEMA: &'static OperationSchema<'static> = {
                static SCHEMA: OperationSchema<'static> =
                    OperationSchema::new(shape_id!("test", "Operation"), &$input, &$output, &[]);
                &SCHEMA
            };
        }
        impl StreamingOperationShape for $name {
            fn deserialize_streaming_input(
                _: &mut dyn ShapeDeserializer,
                _: SdkBody,
                _: SharedServerProtocol,
            ) -> crate::operation::StreamingInputFuture<()> {
                Box::pin(async { Ok(()) })
            }
            fn serialize_streaming_output(_: (), _: &SharedServerProtocol) -> http::Response<BoxBody> {
                http::Response::new(crate::body::empty())
            }
        }
    };
}
operation!(InputOnly, EVENTS, EMPTY);
operation!(OutputOnly, EMPTY, EVENTS);
operation!(Duplex, EVENTS, EVENTS);
operation!(StreamingBlob, BLOBS, BLOBS);
operation!(Ordinary, EMPTY, EMPTY);

async fn check<Op: StreamingOperationShape<Input = (), Output = ()>>(expected: http::StatusCode) {
    let polled = Arc::new(AtomicBool::new(false));
    let called = Arc::new(AtomicBool::new(false));
    let body_polled = polled.clone();
    let body = http_body_util::StreamBody::new(futures_util::stream::poll_fn(move |_| {
        body_polled.store(true, Ordering::SeqCst);
        Poll::Ready(Some(Ok::<_, Infallible>(http_body::Frame::data(Bytes::new()))))
    }));
    let mut request = http::Request::new(body);
    request.extensions_mut().insert(SelectedProtocolOperation::new(
        SharedServerProtocol::serde_only(HttpOnly::default()),
        Op::SCHEMA,
        Default::default(),
    ));
    let handler_called = called.clone();
    let upgrade = DynStreamingUpgrade::<Op, (), _> {
        inner: tower::service_fn(move |_: ((), ())| {
            handler_called.store(true, Ordering::SeqCst);
            async { Ok::<_, Infallible>(()) }
        }),
        _operation: PhantomData,
        _extractors: PhantomData,
    };
    let response = upgrade.oneshot(request).await.unwrap();
    assert_eq!(response.status(), expected);
    assert!(response.into_body().collect().await.unwrap().to_bytes().is_empty());
    assert!(!polled.load(Ordering::SeqCst), "body must remain live and unpolled");
    assert_eq!(called.load(Ordering::SeqCst), expected == http::StatusCode::OK);
}

#[tokio::test]
async fn unsupported_event_directions_reject_before_polling_or_calling_handler() {
    check::<InputOnly>(http::StatusCode::INTERNAL_SERVER_ERROR).await;
    check::<OutputOnly>(http::StatusCode::INTERNAL_SERVER_ERROR).await;
    check::<Duplex>(http::StatusCode::INTERNAL_SERVER_ERROR).await;
}

#[tokio::test]
async fn http_and_streaming_blobs_need_no_event_capability() {
    check::<Ordinary>(http::StatusCode::OK).await;
    check::<StreamingBlob>(http::StatusCode::OK).await;
}

struct EmptyShape;
impl DeserializableShape for EmptyShape {
    fn deserialize(deserializer: &mut dyn ShapeDeserializer) -> Result<Self, DeserializeError> {
        deserializer.read_struct(&EMPTY, &mut |_, _| Ok(()))?;
        Ok(Self)
    }
}
impl SerializableStruct for EmptyShape {
    fn schema(&self) -> &Schema<'_> {
        &EMPTY
    }
    fn serialize_members(
        &self,
        _: &mut dyn aws_smithy_schema::serde::ShapeSerializer,
    ) -> Result<(), aws_smithy_schema::serde::SerdeError> {
        Ok(())
    }
}
struct NonStreaming;
impl OperationShape for NonStreaming {
    const ID: crate::shape_id::ShapeId = Ordinary::ID;
    type Input = EmptyShape;
    type Output = EmptyShape;
    type Error = Infallible;
}
impl SchemaOperationShape for NonStreaming {
    const SCHEMA: &'static OperationSchema<'static> = Ordinary::SCHEMA;
}

#[tokio::test]
async fn schema_upgrades_preserve_request_metadata() {
    for (streaming_upgrade, streaming_input, collect) in [
        (false, false, false),
        (false, false, true),
        (true, false, false),
        (true, false, true),
        (true, true, false),
    ] {
        let extension = Arc::new(String::from("preserved"));
        let weak = Arc::downgrade(&extension);
        let protocol = HttpOnly {
            expected_extension: Some(weak.clone()),
            collect,
        };
        let mut request = http::Request::builder()
            .method("PATCH")
            .uri("/metadata?key=value")
            .header("x-test", "header")
            .body(http_body_util::Full::new(Bytes::from_static(b"payload")))
            .unwrap();
        request.extensions_mut().insert(extension);
        request.extensions_mut().insert(SelectedProtocolOperation::new(
            SharedServerProtocol::serde_only(protocol),
            if streaming_input {
                StreamingBlob::SCHEMA
            } else {
                Ordinary::SCHEMA
            },
            Default::default(),
        ));
        let response = if streaming_upgrade {
            let service = tower::service_fn(|_: ((), ())| async { Ok::<_, Infallible>(()) });
            if streaming_input {
                DynStreamingUpgrade::<StreamingBlob, (), _> {
                    inner: service,
                    _operation: PhantomData,
                    _extractors: PhantomData,
                }
                .oneshot(request)
                .await
                .unwrap()
            } else {
                DynStreamingUpgrade::<Ordinary, (), _> {
                    inner: service,
                    _operation: PhantomData,
                    _extractors: PhantomData,
                }
                .oneshot(request)
                .await
                .unwrap()
            }
        } else {
            DynUpgrade::<NonStreaming, (), _> {
                inner: tower::service_fn(|_: (EmptyShape, ())| async { Ok::<_, Infallible>(EmptyShape) }),
                _operation: PhantomData,
                _extractors: PhantomData,
            }
            .oneshot(request)
            .await
            .unwrap()
        };
        assert_eq!(response.status(), http::StatusCode::OK);
        assert!(
            weak.upgrade().is_none(),
            "extension is released when the request finishes"
        );
    }
}
