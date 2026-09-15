/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use super::*;
use crate::schema::{HttpModeledError, ServerProtocol, SharedServerProtocol};
use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::{shape_id, OperationSchema, Schema, ShapeType};
use bytes::Bytes;
use http_body_util::BodyExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A perfectly usable HTTP protocol that deliberately has no event-stream capability.
#[derive(Debug)]
struct HttpOnly;

impl ServerProtocol for HttpOnly {
    fn build_router(
        &self,
        ctx: crate::routing::RouterBuildContext<'_>,
    ) -> Result<crate::routing::SharedProtocolRouter, crate::routing::RouterBuildError> {
        crate::routing::schema::rest_router::<crate::protocol::rest_json_1::RestJson1>(ctx.targets)
    }
    fn serialize_internal_failure(&self) -> crate::response::Response {
        crate::response::IntoResponse::<crate::protocol::rest_json_1::RestJson1>::into_response(
            crate::runtime_error::InternalFailureException,
        )
    }

    fn protocol_id(&self) -> &'static aws_smithy_schema::ShapeId<'static> {
        static ID: aws_smithy_schema::ShapeId<'static> = shape_id!("test", "httpOnly");
        &ID
    }
    fn reads_request_body(&self, _: &Schema<'_>) -> bool {
        false
    }
    fn deserialize_request<'a>(
        &'a self,
        _: &Schema<'_>,
        _: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
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
static OP: Schema<'static> = Schema::new(shape_id!("test", "Operation"), ShapeType::Operation);

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
                static SCHEMA: OperationSchema<'static> = OperationSchema::new(&OP, &$input, &$output, &[]);
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
        SharedServerProtocol::new(HttpOnly),
        Op::SCHEMA,
    ));
    let handler_called = called.clone();
    let upgrade = StreamingUpgrade::<Op, (), _> {
        inner: tower::service_fn(move |_: ((), ())| {
            handler_called.store(true, Ordering::SeqCst);
            async { Ok::<_, Infallible>(()) }
        }),
        config: RequestBodyCollectionConfig::default(),
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
