/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::DynCodec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::rest_json_1::rejection::RequestRejection;
use crate::protocol::rest_json_1::runtime_error::RuntimeError;
use crate::protocol::rest_json_1::{RestJson1, RestJson1Protocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, stamp_validation_extension,
    ResponseBindings,
};
use super::rest::RestPolicy;
use super::{ServerEventStreamProtocol, ServerProtocol, ServerRequest};

static PROTOCOL_ID: ShapeId<'static> = shape_id!("aws.protocols", "restJson1");
const CONTENT_TYPE: &str = "application/json";
const ERROR_TYPE_HEADER: http::HeaderName = http::HeaderName::from_static("x-amzn-errortype");

/// restJson1 stamps `application/json` on every response nothing else labels, sets no content
/// type on an untyped blob payload, and answers a user-modeled empty output with `{}`.
pub(crate) const POLICY: RestPolicy = RestPolicy {
    codec_content_type: CONTENT_TYPE,
    default_response_content_type: Some(CONTENT_TYPE),
    untyped_blob_payload_content_type: None,
    empty_document: true,
};

impl ServerEventStreamProtocol for RestJson1Protocol {
    fn payload_codec(&self) -> &dyn DynCodec {
        self.inner.codec()
    }

    fn event_stream_media_type(&self) -> &str {
        CONTENT_TYPE
    }

    fn initial_messages_in_frames(&self) -> bool {
        false
    }
}

impl ServerProtocol for RestJson1Protocol {
    fn build_router(
        &self,
        ctx: crate::routing::RouterBuildContext<'_>,
    ) -> Result<crate::routing::SharedProtocolRouter, crate::routing::RouterBuildError> {
        crate::routing::schema::rest_router::<RestJson1>(ctx.targets)
    }
    fn protocol_id(&self) -> &'static ShapeId<'static> {
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
