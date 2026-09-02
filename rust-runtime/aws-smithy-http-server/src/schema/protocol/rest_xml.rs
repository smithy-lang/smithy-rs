/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::sync::LazyLock;

use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::Schema;
use aws_smithy_xml::codec::{XmlCodec, XmlCodecSettings};

use crate::body::BoxBody;
use crate::deserialize::DeserializeError;
use crate::modeled_error::HttpModeledError;
use crate::protocol::rest_xml::RestXml;
use crate::response::IntoResponse;
use crate::schema::protocol::request::{deserialize_rest_request, rest_request_deserializer};
use crate::schema::protocol::response::{
    log_serialize_failure, serialize_modeled_error_response, serialize_operation_response, stamp_error_extension,
    AsSerializable, ResponseBindingMode,
};

use super::{StaticEventStreamProtocol, StaticProtocol};

// ============================================================================
// restXml
// ============================================================================

impl StaticProtocol for RestXml {
    type Codec = XmlCodec;
    type RequestRejection = crate::protocol::rest_xml::rejection::RequestRejection;

    fn codec() -> &'static Self::Codec {
        static CODEC: LazyLock<XmlCodec> = LazyLock::new(|| XmlCodec::new(XmlCodecSettings::default()));
        &CODEC
    }

    fn with_request_deserializer<R>(
        schema: &Schema<'_>,
        parts: &http::request::Parts,
        body: &[u8],
        f: impl FnOnce(&mut dyn ShapeDeserializer) -> Result<R, DeserializeError>,
    ) -> Result<R, Self::RequestRejection> {
        deserialize_rest_request(Self::codec(), "application/xml", true, schema, parts, body, f)
    }

    fn request_deserializer<'a>(
        schema: &Schema<'_>,
        request: &'a http::Request<bytes::Bytes>,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, Self::RequestRejection> {
        rest_request_deserializer(Self::codec(), "application/xml", true, schema, request)
    }

    fn request_rejection_into_response(rejection: Self::RequestRejection) -> http::Response<BoxBody> {
        IntoResponse::<RestXml>::into_response(crate::protocol::rest_xml::runtime_error::RuntimeError::from(rejection))
    }

    fn serialize_response(schema: &Schema<'_>, output: &dyn SerializableStruct) -> http::Response<BoxBody> {
        let result = serialize_operation_response(
            Self::codec(),
            schema,
            output,
            ResponseBindingMode::Rest,
            "application/xml",
            None,
        );
        match result {
            Ok(response) => response,
            Err(err) => {
                log_serialize_failure(&err);
                IntoResponse::<RestXml>::into_response(
                    crate::protocol::rest_xml::runtime_error::RuntimeError::Serialization(crate::Error::new(err)),
                )
            }
        }
    }

    fn serialize_error(error: &dyn HttpModeledError) -> http::Response<BoxBody> {
        // Known divergence, deliberate (2f, fix-forward): today's generated
        // restXml server error bodies are broken (bare `<Error>` envelope no
        // client parses, and the runtime discards pre-rendered
        // validation/framework bodies in favor of a literal `"{}"`).
        // Freezing that behavior would freeze a bug, so the schema path
        // serializes the error structure through the XML codec as-is. See
        // assumptions register B4/B6; gated by its own pinned goldens.
        let schema = error.schema();
        let result = serialize_modeled_error_response(
            Self::codec(),
            schema,
            &AsSerializable(error),
            HttpModeledError::status_code(error),
            ResponseBindingMode::Rest,
            "application/xml",
        );
        match result {
            Ok(response) => stamp_error_extension(response, schema.shape_id().shape_name()),
            Err(err) => {
                log_serialize_failure(&err);
                IntoResponse::<RestXml>::into_response(
                    crate::protocol::rest_xml::runtime_error::RuntimeError::Serialization(crate::Error::new(err)),
                )
            }
        }
    }
}

impl StaticEventStreamProtocol for RestXml {
    const EVENT_PAYLOAD_CONTENT_TYPE: &'static str = "application/xml";
    const EVENT_STREAM_HTTP_CONTENT_TYPE: &'static str = "application/vnd.amazon.eventstream";
    const FRAMES_INITIAL_MESSAGES: bool = false;
}
