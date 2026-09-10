/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! awsJson1.0 and awsJson1.1 share a codec, a rejection type and a runtime error; they differ in
//! the content type and in whether the `__type` discriminator is the full shape ID or the name.

use std::sync::LazyLock;

use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, OperationSchema, Schema, ShapeId};

use crate::protocol::aws_json::rejection::RequestRejection;
use crate::protocol::aws_json::runtime_error::RuntimeError;
use crate::protocol::aws_json_10::AwsJson1_0;
use crate::protocol::aws_json_11::AwsJson1_1;
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::discriminator::{full_shape_id, shape_name_only, WithTypeLast};
use super::request::{check_accept, rpc_request_deserializer};
use super::response::{
    log_serialize_failure, serialize_rpc_modeled_error_response, serialize_rpc_operation_response,
    stamp_error_extension, stamp_validation_extension,
};
use super::{CompiledOperation, RpcOperationState, ServerProtocol, ServerRequest};

fn codec() -> &'static JsonCodec {
    static CODEC: LazyLock<JsonCodec> = LazyLock::new(|| {
        JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(false)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                .build(),
        )
    });
    &CODEC
}

fn serialize_response<P>(
    operation: &OperationSchema<'_>,
    output: &dyn SerializableStruct,
    content_type: &'static str,
) -> Response
where
    RuntimeError: IntoResponse<P>,
{
    // awsJson stamps the content type even on an empty body.
    serialize_rpc_operation_response(codec(), operation, output, content_type, Some(content_type))
        .unwrap_or_else(serialization_failure::<P>)
}

fn serialize_error<P>(
    error: &dyn HttpModeledError,
    content_type: &'static str,
    type_value: for<'s> fn(&'s Schema<'s>) -> &'s str,
) -> Response
where
    RuntimeError: IntoResponse<P>,
{
    let schema = error.schema();
    let framed = WithTypeLast {
        type_value: type_value(schema),
        inner: error,
    };
    serialize_rpc_modeled_error_response(codec(), schema, &framed, error.status_code(), content_type)
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
    ($marker:ty, $protocol_id:expr, $content_type:literal, $type_value:ident) => {
        impl ServerProtocol for $marker {
            type Codec = JsonCodec;
            type OperationState = RpcOperationState;

            fn protocol_id(&self) -> &'static ShapeId<'static> {
                static PROTOCOL_ID: ShapeId<'static> = $protocol_id;
                &PROTOCOL_ID
            }

            fn codec(&self) -> &'static JsonCodec {
                codec()
            }

            fn deserialize_request<'a>(
                &'a self,
                operation: &'a CompiledOperation<RpcOperationState>,
                request: &'a ServerRequest,
            ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
                // awsJson responses always carry the protocol content type, so every generated
                // `FromRequest` runs the `Accept` gate against it.
                check_accept(&request.headers, $content_type)?;
                rpc_request_deserializer(self.codec(), $content_type, operation.schema().input(), request)
            }

            fn serialize_response(
                &self,
                operation: &CompiledOperation<RpcOperationState>,
                output: &dyn SerializableStruct,
            ) -> Response {
                serialize_response::<$marker>(operation.schema(), output, $content_type)
            }

            fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
                serialize_error::<$marker>(error, $content_type, $type_value)
            }

            /// awsJson's `From<RequestRejection>` collapses every transport failure — `Accept`
            /// and `Content-Type` mismatches included — into a 400 `Serialization`; routing
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
                }
            }
        }
    };
}

aws_json_protocol!(
    AwsJson1_0,
    shape_id!("aws.protocols", "awsJson1_0"),
    "application/x-amz-json-1.0",
    full_shape_id
);
aws_json_protocol!(
    AwsJson1_1,
    shape_id!("aws.protocols", "awsJson1_1"),
    "application/x-amz-json-1.1",
    shape_name_only
);
