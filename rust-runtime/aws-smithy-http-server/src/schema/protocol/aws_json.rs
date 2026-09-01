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
use crate::schema::protocol::request::deserialize_rpc_request;
use crate::schema::protocol::response::{
    assemble_response, log_serialize_failure, stamp_error_extension, AsSerializable,
};
use crate::schema::response_bindings::{resolve_status, serialize_response_parts, ResponseValueKind};

use super::{EventStreamProtocol, ServerProtocol};

static AMZ_JSON_10_MIME: LazyLock<mime::Mime> =
    LazyLock::new(|| "application/x-amz-json-1.0".parse().expect("valid mime"));
static AMZ_JSON_11_MIME: LazyLock<mime::Mime> =
    LazyLock::new(|| "application/x-amz-json-1.1".parse().expect("valid mime"));

// ============================================================================
// awsJson 1.0 / 1.1
// ============================================================================

fn aws_json_codec() -> &'static JsonCodec {
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

macro_rules! aws_json_impl {
    ($marker:ty, $content_type:literal, $mime:ident, $type_value:ident) => {
        impl ServerProtocol for $marker {
            type Codec = JsonCodec;
            type RequestRejection = crate::protocol::aws_json::rejection::RequestRejection;

            fn codec() -> &'static Self::Codec {
                aws_json_codec()
            }

            fn with_request_deserializer<R>(
                schema: &Schema<'_>,
                _output_schema: &Schema<'_>,
                parts: &http::request::Parts,
                body: &[u8],
                f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
            ) -> Result<R, Self::RequestRejection> {
                if !crate::protocol::accept_header_classifier(&parts.headers, &$mime) {
                    return Err(Self::RequestRejection::NotAcceptable);
                }
                deserialize_rpc_request(Self::codec(), $content_type, schema, parts, body, f)
            }

            fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
                let result = serialize_response_parts(
                    Self::codec(),
                    schema,
                    output,
                    false,
                    ResponseValueKind::OperationOutput,
                )
                .and_then(|split| {
                    let status = resolve_status(split.status, schema);
                    assemble_response(split, status, $content_type, Some($content_type))
                });
                match result {
                    Ok(response) => response,
                    Err(err) => {
                        log_serialize_failure(&err);
                        IntoResponse::<$marker>::into_response(
                            crate::protocol::aws_json::runtime_error::RuntimeError::Serialization(crate::Error::new(
                                err,
                            )),
                        )
                    }
                }
            }

            fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
                let schema = error.schema();
                // `__type` written after the modeled members (legacy order).
                let wrapper = WithTypeLast {
                    type_value: $type_value(schema),
                    inner: &AsSerializable(error),
                };
                let result = serialize_response_parts(
                    Self::codec(),
                    schema,
                    &wrapper,
                    false,
                    ResponseValueKind::ModeledError,
                )
                .and_then(|split| assemble_response(split, error.status_code(), $content_type, None));
                match result {
                    Ok(response) => stamp_error_extension(response, schema.shape_id().shape_name()),
                    Err(err) => {
                        log_serialize_failure(&err);
                        IntoResponse::<$marker>::into_response(
                            crate::protocol::aws_json::runtime_error::RuntimeError::Serialization(crate::Error::new(
                                err,
                            )),
                        )
                    }
                }
            }
        }
    };
}

aws_json_impl!(
    AwsJson1_0,
    "application/x-amz-json-1.0",
    AMZ_JSON_10_MIME,
    full_shape_id
);
aws_json_impl!(
    AwsJson1_1,
    "application/x-amz-json-1.1",
    AMZ_JSON_11_MIME,
    shape_name_only
);

impl EventStreamProtocol for AwsJson1_0 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/x-amz-json-1.0";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}

impl EventStreamProtocol for AwsJson1_1 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/x-amz-json-1.1";
    const FRAMES_INITIAL_MESSAGES: bool = true;
}
