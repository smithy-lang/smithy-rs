/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! awsJson1.0 and awsJson1.1 share a codec, a rejection type and a runtime error; they differ in
//! the content type and in whether the `__type` discriminator is the full shape ID or the name.

use aws_smithy_json::codec::JsonCodec;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::aws_json::rejection::RequestRejection;
use crate::protocol::aws_json::runtime_error::RuntimeError;
use crate::protocol::aws_json_10::AwsJson1_0;
use crate::protocol::aws_json_11::AwsJson1_1;
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::discriminator::{BodyDiscriminator, TypeValue};
use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::{BodyDirective, EventStreamFraming, MetadataRoutedProtocol, ServerProtocol};

/// Stateful schema-driven AWS JSON 1.0 protocol implementation.
#[derive(Debug)]
pub struct AwsJson1_0Protocol {
    pub(crate) inner: crate::schema::protocol::rpc::RpcProtocol<aws_smithy_json::codec::JsonCodec>,
}

impl Default for AwsJson1_0Protocol {
    fn default() -> Self {
        Self {
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                schema_codec(),
                "application/x-amz-json-1.0",
                Some("application/x-amz-json-1.0"),
                crate::schema::protocol::rpc::RpcAccept::Always,
                crate::schema::protocol::rpc::RpcStreaming::CodecContentType,
            ),
        }
    }
}

/// Stateful schema-driven AWS JSON 1.1 protocol implementation.
#[derive(Debug)]
pub struct AwsJson1_1Protocol {
    pub(crate) inner: crate::schema::protocol::rpc::RpcProtocol<aws_smithy_json::codec::JsonCodec>,
}

impl Default for AwsJson1_1Protocol {
    fn default() -> Self {
        Self {
            inner: crate::schema::protocol::rpc::RpcProtocol::new(
                schema_codec(),
                "application/x-amz-json-1.1",
                Some("application/x-amz-json-1.1"),
                crate::schema::protocol::rpc::RpcAccept::Always,
                crate::schema::protocol::rpc::RpcStreaming::CodecContentType,
            ),
        }
    }
}

fn schema_codec() -> aws_smithy_json::codec::JsonCodec {
    aws_smithy_json::codec::JsonCodec::new(
        aws_smithy_json::codec::JsonCodecSettings::builder()
            .use_json_name(false)
            .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
            .enforce_strictness(true)
            .allow_integral_float_numbers(true)
            .strict_timestamp_formats(true)
            .build(),
    )
}

fn serialize_error<P>(
    codec: &JsonCodec,
    error: &dyn HttpModeledError,
    content_type: &'static str,
    discriminator: BodyDiscriminator,
) -> Response
where
    RuntimeError: IntoResponse<P>,
{
    let schema = error.schema();
    let framed = discriminator.frame(schema, error);
    serialize_modeled_error_response(
        codec,
        schema,
        &framed,
        error.status_code(),
        ResponseBindings::BodyOnly,
        content_type,
    )
    .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
    .unwrap_or_else(serialization_failure::<P>)
}

fn serialization_failure<P>(err: SerdeError) -> Response
where
    RuntimeError: IntoResponse<P>,
{
    log_serialize_failure(&err);
    IntoResponse::<P>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}

macro_rules! aws_json_protocol {
    ($protocol:ty, $marker:ty, $protocol_id:expr, $content_type:literal, $type_value:expr) => {
        impl MetadataRoutedProtocol for $protocol {
            fn from_build_context(
                _ctx: &crate::schema::ProtocolBuildContext<'_>,
            ) -> Result<Self, crate::schema::routing::RouterBuildError> {
                Ok(Self::default())
            }

            fn build_router(
                &self,
                ctx: crate::schema::routing::RouterBuildContext<'_>,
            ) -> Result<
                impl crate::schema::routing::MetadataProtocolRouter + 'static + use<>,
                crate::schema::routing::RouterBuildError,
            > {
                crate::schema::routing::aws_json_router(&ctx, $content_type)
            }

            fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
                Some(EventStreamFraming::new(self.inner.codec(), "application/json").initial_messages_in_frames(true))
            }
        }

        impl ServerProtocol for $protocol {
            fn protocol_id(&self) -> &'static ShapeId<'static> {
                static PROTOCOL_ID: ShapeId<'static> = $protocol_id;
                &PROTOCOL_ID
            }

            fn validate_request_headers(
                &self,
                operation: &crate::schema::OperationSchema<'_>,
                headers: &Headers,
            ) -> Result<(), DeserializeError> {
                self.inner.check_accept(operation.output(), headers)
            }

            fn request_body_requirement(&self, operation: &crate::schema::OperationSchema<'_>) -> BodyDirective {
                if self.inner.reads_request_body(operation.input()) {
                    BodyDirective::Collect
                } else {
                    BodyDirective::Skip
                }
            }

            fn deserialize_request<'a>(
                &'a self,
                input: &Schema<'_>,
                request: &'a aws_smithy_runtime_api::http::Request<bytes::Bytes>,
            ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
                self.inner.deserialize_request(input, request)
            }

            fn serialize_response(&self, output: &Schema<'_>, value: &dyn SerializableStruct) -> Response {
                self.inner
                    .serialize_response(output, value)
                    .unwrap_or_else(serialization_failure::<$marker>)
            }

            fn serialize_streaming_response(
                &self,
                output: &Schema<'_>,
                value: &dyn SerializableStruct,
                body: BoxBody,
            ) -> Response {
                self.inner
                    .serialize_streaming_response(output, value, body)
                    .unwrap_or_else(serialization_failure::<$marker>)
            }

            fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
                serialize_error::<$marker>(
                    self.inner.codec(),
                    error,
                    $content_type,
                    BodyDiscriminator { value: $type_value },
                )
            }

            /// awsJson's `From<RequestRejection>` collapses every transport failure, `Accept`
            /// and `Content-Type` mismatches included, into a 400 `Serialization`; routing
            /// through that same `From` preserves the collapse. A constraint violation answers
            /// with the modeled validation error.
            fn serialize_rejection(&self, err: DeserializeError) -> Response {
                match err {
                    DeserializeError::Serde(err) => {
                        IntoResponse::<$marker>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
                    }
                    DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<$marker>::into_response(
                        RuntimeError::from(RequestRejection::MissingContentType(*reason)),
                    ),
                    DeserializeError::NotAcceptable => {
                        IntoResponse::<$marker>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
                    }
                    DeserializeError::ConstraintViolation(err) => {
                        stamp_validation_extension(self.serialize_error(&*err))
                    }
                    DeserializeError::InternalFailure(err) => {
                        IntoResponse::<$marker>::into_response(RuntimeError::InternalFailure(err))
                    }
                }
            }
        }
    };
}

aws_json_protocol!(
    AwsJson1_0Protocol,
    AwsJson1_0,
    shape_id!("aws.protocols", "awsJson1_0"),
    "application/x-amz-json-1.0",
    TypeValue::FullShapeId
);
aws_json_protocol!(
    AwsJson1_1Protocol,
    AwsJson1_1,
    shape_id!("aws.protocols", "awsJson1_1"),
    "application/x-amz-json-1.1",
    TypeValue::ShapeName
);
