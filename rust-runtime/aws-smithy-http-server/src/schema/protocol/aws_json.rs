/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! awsJson1.0 and awsJson1.1 share a codec, a rejection type and a runtime error; they differ in
//! the content type and in whether the `__type` discriminator is the full shape ID or the name.

use aws_smithy_json::codec::JsonCodec;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::DynCodec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::aws_json::rejection::RequestRejection;
use crate::protocol::aws_json::runtime_error::RuntimeError;
use crate::protocol::aws_json_10::{AwsJson1_0, AwsJson1_0Protocol};
use crate::protocol::aws_json_11::{AwsJson1_1, AwsJson1_1Protocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::discriminator::{BodyDiscriminator, TypeValue};
use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::{ServerEventStreamProtocol, ServerProtocol, ServerRequest};

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
        impl ServerEventStreamProtocol for $protocol {
            fn payload_codec(&self) -> &dyn DynCodec {
                self.inner.codec()
            }

            fn event_stream_media_type(&self) -> &str {
                "application/json"
            }

            fn initial_messages_in_frames(&self) -> bool {
                true
            }
        }

        impl ServerProtocol for $protocol {
            fn build_router(
                &self,
                ctx: crate::routing::RouterBuildContext<'_>,
            ) -> Result<crate::routing::SharedProtocolRouter, crate::routing::RouterBuildError> {
                crate::routing::schema::aws_json_router::<$marker>(&ctx)
            }
            fn protocol_id(&self) -> &'static ShapeId<'static> {
                static PROTOCOL_ID: ShapeId<'static> = $protocol_id;
                &PROTOCOL_ID
            }

            fn event_stream(&self) -> Option<&dyn ServerEventStreamProtocol> {
                Some(self)
            }

            fn check_accept(&self, output: &Schema<'_>, headers: &Headers) -> Result<(), DeserializeError> {
                self.inner.check_accept(output, headers)
            }

            fn reads_request_body(&self, input: &Schema<'_>) -> bool {
                self.inner.reads_request_body(input)
            }

            fn deserialize_request<'a>(
                &'a self,
                input: &Schema<'_>,
                request: &'a ServerRequest,
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
