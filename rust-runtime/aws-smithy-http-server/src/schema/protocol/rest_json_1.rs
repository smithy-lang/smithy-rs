/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::rest_json_1::rejection::RequestRejection;
use crate::protocol::rest_json_1::runtime_error::RuntimeError;
use crate::protocol::rest_json_1::RestJson1;
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::rest::RestPolicy;
use super::{BodyDirective, EventStreamFraming, MetadataRoutedProtocol, ServerProtocol};

/// Stateful schema-driven restJson1 protocol implementation.
#[derive(Debug)]
/// Defaults to validating escape syntax in skipped strings without decoding Unicode.
/// Set `customizationConfig.protocols` for this protocol to `{"validateSkippedValues":true}`
/// to skip escape validation, matching legacy smithy-rs servers.
pub struct RestJson1Protocol {
    pub(crate) inner: crate::schema::protocol::rest::RestProtocol<aws_smithy_json::codec::JsonCodec>,
}

impl Default for RestJson1Protocol {
    fn default() -> Self {
        Self::new(false)
    }
}

impl RestJson1Protocol {
    fn new(validate_skipped_values: bool) -> Self {
        Self {
            inner: crate::schema::protocol::rest::RestProtocol::new(
                aws_smithy_json::codec::JsonCodec::new(
                    aws_smithy_json::codec::JsonCodecSettings::builder()
                        .use_json_name(true)
                        .default_timestamp_format(aws_smithy_types::date_time::Format::EpochSeconds)
                        .enforce_strictness(true)
                        .validate_skipped_values(validate_skipped_values)
                        .allow_integral_float_numbers(true)
                        .strict_timestamp_formats(true)
                        .build(),
                ),
                crate::schema::protocol::rest_json_1::POLICY,
            ),
        }
    }
}

static PROTOCOL_ID: ShapeId<'static> = shape_id!("aws.protocols", "restJson1");
const CONTENT_TYPE: &str = "application/json";
const ERROR_TYPE_HEADER: http::HeaderName = http::HeaderName::from_static("x-amzn-errortype");

/// restJson1 stamps `application/json` on every response nothing else labels, sets no content
/// type on an untyped blob payload, and answers a user-modeled empty output with `{}`.
pub(crate) const POLICY: RestPolicy = RestPolicy {
    codec_content_type: CONTENT_TYPE,
    request_content_type_aliases: &[],
    default_response_content_type: Some(CONTENT_TYPE),
    untyped_blob_payload_content_type: None,
    empty_document: true,
};

impl MetadataRoutedProtocol for RestJson1Protocol {
    fn from_build_context(
        ctx: &crate::schema::ProtocolBuildContext<'_>,
    ) -> Result<Self, crate::schema::routing::RouterBuildError> {
        let mut protocol =
            Self::new(crate::schema::settings::get::<bool>(ctx.settings, "validateSkippedValues")?.unwrap_or(false));
        protocol.inner.prepare_response_plans(ctx.service);
        Ok(protocol)
    }

    fn build_router(
        &self,
        ctx: crate::schema::routing::RouterBuildContext<'_>,
    ) -> Result<
        impl crate::schema::routing::MetadataProtocolRouter + 'static + use<>,
        crate::schema::routing::RouterBuildError,
    > {
        crate::schema::routing::rest_router(ctx.targets, CONTENT_TYPE, &[])
    }

    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
        Some(EventStreamFraming::new(self.inner.codec(), CONTENT_TYPE))
    }
}

impl ServerProtocol for RestJson1Protocol {
    fn protocol_id(&self) -> &'static ShapeId<'static> {
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
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Response {
        self.inner
            .serialize_streaming_response(output, value, body)
            .unwrap_or_else(serialization_failure)
    }

    fn serialize_error(&self, error: &dyn HttpModeledError) -> Response {
        let schema = error.schema();
        let name = schema.shape_id().shape_name();
        let result = serialize_modeled_error_response(
            self.inner.codec(),
            schema,
            error,
            error.status_code(),
            ResponseBindings::Rest,
            CONTENT_TYPE,
        );
        match result {
            Ok(mut response) => {
                // The discriminator travels in the header, as the shape name only.
                if let Ok(value) = http::HeaderValue::try_from(name) {
                    response.headers_mut().insert(ERROR_TYPE_HEADER, value);
                }
                stamp_error_extension(response, name)
            }
            Err(err) => serialization_failure(err),
        }
    }

    /// restJson1 is the only protocol that keeps `Accept` and `Content-Type` failures distinct:
    /// 406 and 415, per its `From<RequestRejection>` mapping. Transport failures answer with the
    /// `RuntimeError` responses; a constraint violation with the modeled validation error.
    fn serialize_rejection(&self, err: DeserializeError) -> Response {
        match err {
            DeserializeError::Serde(err) => {
                IntoResponse::<RestJson1>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
            }
            DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<RestJson1>::into_response(
                RuntimeError::from(RequestRejection::MissingContentType(*reason)),
            ),
            DeserializeError::NotAcceptable => {
                IntoResponse::<RestJson1>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
            }
            DeserializeError::ConstraintViolation(err) => stamp_validation_extension(self.serialize_error(&*err)),
            DeserializeError::InternalFailure(err) => {
                IntoResponse::<RestJson1>::into_response(RuntimeError::InternalFailure(err))
            }
        }
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RestJson1>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}
