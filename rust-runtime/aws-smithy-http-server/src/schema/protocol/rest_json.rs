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
use crate::protocol::rest_json_1::RestJson1;
use crate::response::IntoResponse;
use crate::schema::protocol::request::{deserialize_rest_request, rest_request_deserializer};
use crate::schema::protocol::response::{
    log_serialize_failure, serialize_modeled_error_response, serialize_operation_response, stamp_error_extension,
    AsSerializable, ResponseBindingMode,
};

use super::{StaticEventStreamProtocol, StaticProtocol};

// ============================================================================
// restJson1
// ============================================================================

impl StaticProtocol for RestJson1 {
    type Codec = JsonCodec;
    type RequestRejection = crate::protocol::rest_json_1::rejection::RequestRejection;

    fn codec() -> &'static Self::Codec {
        rest_json_1_codec()
    }

    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection> {
        deserialize_rest_request(Self::codec(), "application/json", true, schema, parts, body, f)
    }

    fn request_deserializer<'a>(
        schema: &Schema<'_>,
        request: &'a http::Request<bytes::Bytes>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, Self::RequestRejection> {
        rest_json_1_request_deserializer(schema, request)
    }

    fn request_rejection_into_response(rejection: Self::RequestRejection) -> http::Response<BoxBody> {
        IntoResponse::<RestJson1>::into_response(crate::protocol::rest_json_1::runtime_error::RuntimeError::from(
            rejection,
        ))
    }

    fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
        rest_json_1_serialize_response(schema, output)
    }

    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
        rest_json_1_serialize_error(error)
    }
}

pub(crate) fn rest_json_1_codec() -> &'static JsonCodec {
    static CODEC: LazyLock<JsonCodec> = LazyLock::new(|| {
        JsonCodec::new(
            JsonCodecSettings::builder()
                .use_json_name(true)
                .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                // Server semantics: `@timestampFormat` is enforced, not
                // coerced (Smithy malformed-timestamp protocol tests).
                .build(),
        )
    });
    &CODEC
}

pub(crate) fn rest_json_1_request_deserializer<'a>(
    schema: &Schema<'_>,
    request: &'a http::Request<bytes::Bytes>,
) -> Result<Box<dyn ShapeDeserializer + 'a>, crate::protocol::rest_json_1::rejection::RequestRejection> {
    rest_request_deserializer(rest_json_1_codec(), "application/json", true, schema, request)
}

pub(crate) fn rest_json_1_serialize_response(
    schema: &Schema<'_>,
    output: &dyn SerializableStruct,
) -> http::Response<BoxBody> {
    let result = serialize_operation_response(
        rest_json_1_codec(),
        schema,
        output,
        ResponseBindingMode::Rest,
        "application/json",
        None,
    );
    match result {
        Ok(response) => response,
        Err(err) => {
            log_serialize_failure(&err);
            IntoResponse::<RestJson1>::into_response(
                crate::protocol::rest_json_1::runtime_error::RuntimeError::Serialization(crate::Error::new(err)),
            )
        }
    }
}

pub(crate) fn rest_json_1_serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
    let schema = error.schema();
    let name = schema.shape_id().shape_name();
    // restJson1 carries no body discriminator; the error name travels in
    // the `x-amzn-errortype` header.
    let result = serialize_modeled_error_response(
        rest_json_1_codec(),
        schema,
        &AsSerializable(error),
        HttpModeledError::status_code(error),
        ResponseBindingMode::Rest,
        "application/json",
    );
    match result {
        Ok(mut response) => {
            // Shape name only — the settled post-#1982 behavior. The
            // legacy hard-coded `ValidationException` header for custom
            // validation shapes was a confirmed bug (2f); the schema path
            // emits the actual shape name.
            if let Ok(value) = http::HeaderValue::try_from(name) {
                response
                    .headers_mut()
                    .insert(http::HeaderName::from_static("x-amzn-errortype"), value);
            }
            stamp_error_extension(response, name)
        }
        Err(err) => {
            log_serialize_failure(&err);
            IntoResponse::<RestJson1>::into_response(
                crate::protocol::rest_json_1::runtime_error::RuntimeError::Serialization(crate::Error::new(err)),
            )
        }
    }
}

impl StaticEventStreamProtocol for RestJson1 {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/json";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/vnd.amazon.eventstream";
    const FRAMES_INITIAL_MESSAGES: bool = false;
}
