/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::LazyLock;

use aws_smithy_json::codec::{JsonCodec, JsonCodecSettings};
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::Schema;

use crate::body::BoxBody;
use crate::deserialize::DeserializeError;
use crate::modeled_error::HttpModeledError;
use crate::protocol::aws_json_10::AwsJson1_0;
use crate::protocol::aws_json_11::AwsJson1_1;
use crate::response::IntoResponse;
use crate::schema::protocol::discriminator::{full_shape_id, shape_name_only, WithTypeLast};
use crate::schema::protocol::request::{deserialize_rpc_request, rpc_request_deserializer};
use crate::schema::protocol::response::{
    log_serialize_failure, serialize_modeled_error_response, serialize_operation_response, stamp_error_extension,
    AsSerializable, ResponseBindingMode,
};

use super::{StaticEventStreamProtocol, StaticProtocol};

// ============================================================================
// awsJson 1.0 / 1.1
// ============================================================================

pub(crate) fn aws_json_codec() -> &'static JsonCodec {
    static CODEC: LazyLock<JsonCodec> = LazyLock::new(|| {
        JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(false)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                // Server semantics: `@timestampFormat` is enforced, not
                // coerced (Smithy malformed-timestamp protocol tests).
                .build(),
        )
    });
    &CODEC
}

pub(crate) fn aws_json_request_deserializer<'a>(
    content_type: &'static str,
    schema: &Schema<'_>,
    request: &'a http::Request<bytes::Bytes>,
) -> Result<Box<dyn ShapeDeserializer + 'a>, crate::protocol::aws_json::rejection::RequestRejection> {
    rpc_request_deserializer(aws_json_codec(), content_type, schema, request)
}

pub(crate) fn aws_json_10_serialize_response(
    schema: &Schema<'_>,
    output: &dyn SerializableStruct,
) -> http::Response<BoxBody> {
    aws_json_serialize_response::<AwsJson1_0>(schema, output, "application/x-amz-json-1.0")
}

pub(crate) fn aws_json_11_serialize_response(
    schema: &Schema<'_>,
    output: &dyn SerializableStruct,
) -> http::Response<BoxBody> {
    aws_json_serialize_response::<AwsJson1_1>(schema, output, "application/x-amz-json-1.1")
}

fn aws_json_serialize_response<P>(
    schema: &Schema<'_>,
    output: &dyn SerializableStruct,
    content_type: &'static str,
) -> http::Response<BoxBody>
where
    crate::protocol::aws_json::runtime_error::RuntimeError: IntoResponse<P>,
{
    let result = serialize_operation_response(
        aws_json_codec(),
        schema,
        output,
        ResponseBindingMode::BodyOnly,
        content_type,
        Some(content_type),
    );
    match result {
        Ok(response) => response,
        Err(err) => {
            log_serialize_failure(&err);
            IntoResponse::<P>::into_response(crate::protocol::aws_json::runtime_error::RuntimeError::Serialization(
                crate::Error::new(err),
            ))
        }
    }
}

pub(crate) fn aws_json_10_serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
    aws_json_serialize_error::<AwsJson1_0>(error, "application/x-amz-json-1.0", full_shape_id)
}

pub(crate) fn aws_json_11_serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
    aws_json_serialize_error::<AwsJson1_1>(error, "application/x-amz-json-1.1", shape_name_only)
}

fn aws_json_serialize_error<P>(
    error: &dyn HttpModeledError,
    content_type: &'static str,
    type_value: for<'s> fn(&'s Schema<'s>) -> &'s str,
) -> http::Response<BoxBody>
where
    crate::protocol::aws_json::runtime_error::RuntimeError: IntoResponse<P>,
{
    let schema = error.schema();
    let wrapper = WithTypeLast {
        type_value: type_value(schema),
        inner: &AsSerializable(error),
    };
    let result = serialize_modeled_error_response(
        aws_json_codec(),
        schema,
        &wrapper,
        HttpModeledError::status_code(error),
        ResponseBindingMode::BodyOnly,
        content_type,
    );
    match result {
        Ok(response) => stamp_error_extension(response, schema.shape_id().shape_name()),
        Err(err) => {
            log_serialize_failure(&err);
            IntoResponse::<P>::into_response(crate::protocol::aws_json::runtime_error::RuntimeError::Serialization(
                crate::Error::new(err),
            ))
        }
    }
}

macro_rules! aws_json_impl {
    ($marker:ty, $content_type:literal, $type_value:ident) => {
        impl StaticProtocol for $marker {
            type Codec = JsonCodec;
            type RequestRejection = crate::protocol::aws_json::rejection::RequestRejection;

            fn codec() -> &'static Self::Codec {
                aws_json_codec()
            }

            fn with_request_deserializer<R>(
                schema: &Schema<'_>,
                parts: &http::request::Parts,
                body: &[u8],
                f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
            ) -> Result<R, Self::RequestRejection> {
                deserialize_rpc_request(Self::codec(), $content_type, schema, parts, body, f)
            }

            fn request_deserializer<'a>(
                schema: &Schema<'_>,
                request: &'a http::Request<bytes::Bytes>,
            ) -> Result<Box<dyn ShapeDeserializer + 'a>, Self::RequestRejection> {
                aws_json_request_deserializer($content_type, schema, request)
            }

            fn request_rejection_into_response(rejection: Self::RequestRejection) -> http::Response<BoxBody> {
                IntoResponse::<$marker>::into_response(crate::protocol::aws_json::runtime_error::RuntimeError::from(
                    rejection,
                ))
            }

            fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
                aws_json_serialize_response::<$marker>(schema, output, $content_type)
            }

            fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
                aws_json_serialize_error::<$marker>(error, $content_type, $type_value)
            }
        }
    };
}

aws_json_impl!(AwsJson1_0, "application/x-amz-json-1.0", full_shape_id);
aws_json_impl!(AwsJson1_1, "application/x-amz-json-1.1", shape_name_only);

impl StaticEventStreamProtocol for AwsJson1_0 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/x-amz-json-1.0";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}

impl StaticEventStreamProtocol for AwsJson1_1 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/x-amz-json-1.1";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}
