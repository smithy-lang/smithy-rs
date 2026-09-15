/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::DynCodec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::rest_xml::rejection::RequestRejection;
use crate::protocol::rest_xml::runtime_error::RuntimeError;
use crate::protocol::rest_xml::{RestXml, RestXmlProtocol};
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, ResponseBindings,
};
use super::rest::RestPolicy;
use super::{ServerEventStreamProtocol, ServerProtocol, ServerRequest};

static PROTOCOL_ID: ShapeId<'static> = shape_id!("aws.protocols", "restXml");
const CONTENT_TYPE: &str = "application/xml";

/// restXml labels a response only when the output schema binds something to the body, gives an
/// untyped blob payload `application/octet-stream`, and sends an empty body for an output with
/// no body members whether or not the user modeled it.
pub(crate) const POLICY: RestPolicy = RestPolicy {
    codec_content_type: CONTENT_TYPE,
    default_response_content_type: None,
    untyped_blob_payload_content_type: Some("application/octet-stream"),
    empty_document: false,
};

impl ServerEventStreamProtocol for RestXmlProtocol {
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

impl ServerProtocol for RestXmlProtocol {
    fn build_router(
        &self,
        ctx: crate::routing::RouterBuildContext<'_>,
    ) -> Result<crate::routing::SharedProtocolRouter, crate::routing::RouterBuildError> {
        crate::routing::schema::rest_router::<RestXml>(ctx.targets)
    }
    fn serialize_internal_failure(&self) -> Response {
        crate::response::IntoResponse::<RestXml>::into_response(crate::runtime_error::InternalFailureException)
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
        // restXml carries no discriminator: the error structure is the body.
        let schema = error.schema();
        serialize_modeled_error_response(
            self.inner.codec(),
            schema,
            error,
            error.status_code(),
            ResponseBindings::Rest,
            CONTENT_TYPE,
        )
        .map(|response| stamp_error_extension(response, schema.shape_id().shape_name()))
        .unwrap_or_else(serialization_failure)
    }

    /// restXml keeps 415 for `Content-Type` failures, but its `From<RequestRejection>` has no
    /// `NotAcceptable` arm, so an `Accept` mismatch falls through to a 400 `Serialization`, NOT
    /// a 406. And its `RuntimeError` response drops the validation body entirely: every runtime
    /// error, constraint violations included, answers with the literal `{}` body, so a constraint
    /// violation must NOT serialize the modeled error here. Both are the protocol's wire contract.
    fn serialize_rejection(&self, err: DeserializeError) -> Response {
        match err {
            DeserializeError::Serde(err) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
            }
            DeserializeError::UnsupportedMediaType(reason) => IntoResponse::<RestXml>::into_response(
                RuntimeError::from(RequestRejection::MissingContentType(*reason)),
            ),
            DeserializeError::NotAcceptable => {
                IntoResponse::<RestXml>::into_response(RuntimeError::from(RequestRejection::NotAcceptable))
            }
            // The smuggled reason string never reaches the wire; legacy renders `{}` regardless.
            DeserializeError::ConstraintViolation(_) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::Validation(String::new()))
            }
        }
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}
