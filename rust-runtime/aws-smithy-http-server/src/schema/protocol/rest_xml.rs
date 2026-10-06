/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};

use crate::body::BoxBody;
use crate::protocol::rest_xml::rejection::RequestRejection;
use crate::protocol::rest_xml::runtime_error::RuntimeError;
use crate::protocol::rest_xml::RestXml;
use crate::response::{IntoResponse, Response};
use crate::schema::{DeserializeError, HttpModeledError};

use super::response::{
    log_serialize_failure, serialize_modeled_error_response, stamp_error_extension, ResponseBindings,
};
use super::rest::RestPolicy;
use super::{BodyDirective, EventStreamFraming, MetadataRoutedProtocol, ServerProtocol};

/// Stateful schema-driven restXml protocol implementation.
///
/// Wrapped collections read only their modeled item and entry names. Requests accept
/// `application/xml`, and parsing checks the modeled root while retaining the legacy
/// parser's recovery from malformed XML.
#[derive(Debug)]
pub struct RestXmlProtocol {
    pub(crate) inner: crate::schema::protocol::rest::RestProtocol<aws_smithy_xml::codec::XmlCodec>,
}

impl RestXmlProtocol {
    fn new(strict_collection_element_names: bool, validate_document: bool, accept_text_xml: bool) -> Self {
        Self {
            inner: crate::schema::protocol::rest::RestProtocol::new(
                aws_smithy_xml::codec::XmlCodec::new(
                    aws_smithy_xml::codec::XmlCodecSettings::builder()
                        .error_root_name("Error")
                        .validate_root_name(true)
                        .validate_document(validate_document)
                        .strict_collection_element_names(strict_collection_element_names)
                        .build(),
                ),
                RestPolicy {
                    request_content_type_aliases: if accept_text_xml {
                        REQUEST_CONTENT_TYPE_ALIASES
                    } else {
                        &[]
                    },
                    ..POLICY
                },
            ),
        }
    }
}

impl Default for RestXmlProtocol {
    fn default() -> Self {
        Self::new(true, false, false)
    }
}

static PROTOCOL_ID: ShapeId<'static> = shape_id!("aws.protocols", "restXml");
const CONTENT_TYPE: &str = "application/xml";

/// Media types a request may use for an XML body besides [`CONTENT_TYPE`].
const REQUEST_CONTENT_TYPE_ALIASES: &[&str] = &["text/xml"];

/// restXml labels a response only when the output schema binds something to the body, gives an
/// untyped blob payload `application/octet-stream`, and sends an empty body for an output with
/// no body members whether or not the user modeled it.
pub(crate) const POLICY: RestPolicy = RestPolicy {
    codec_content_type: CONTENT_TYPE,
    request_content_type_aliases: &[],
    default_response_content_type: None,
    untyped_blob_payload_content_type: Some("application/octet-stream"),
    empty_document: false,
};

impl MetadataRoutedProtocol for RestXmlProtocol {
    fn from_build_context(
        ctx: &crate::schema::ProtocolBuildContext<'_>,
    ) -> Result<Self, crate::schema::routing::RouterBuildError> {
        let mut protocol = Self::new(
            crate::schema::settings::get::<bool>(ctx.settings, "strictCollectionElementNames")?.unwrap_or(true),
            crate::schema::settings::get::<bool>(ctx.settings, "validateDocument")?.unwrap_or(false),
            crate::schema::settings::get::<bool>(ctx.settings, "acceptTextXml")?.unwrap_or(false),
        );
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
        crate::schema::routing::rest_router(
            ctx.targets,
            CONTENT_TYPE,
            self.inner.policy().request_content_type_aliases,
        )
    }

    fn event_stream_framing(&self) -> Option<EventStreamFraming<'_>> {
        Some(EventStreamFraming::new(self.inner.codec(), CONTENT_TYPE).exception_http_bindings(true))
    }
}

impl ServerProtocol for RestXmlProtocol {
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

    fn serialize_routing_error(&self, err: &crate::schema::routing::RoutingError) -> Response {
        use crate::protocol::rest::router::Error;
        use crate::schema::routing::RoutingErrorKind;
        let error = match err.kind() {
            RoutingErrorKind::MethodNotAllowed => Error::MethodNotAllowed,
            _ => Error::NotFound,
        };
        IntoResponse::<RestXml>::into_response(error)
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
            DeserializeError::InternalFailure(err) => {
                IntoResponse::<RestXml>::into_response(RuntimeError::InternalFailure(err))
            }
        }
    }
}

fn serialization_failure(err: SerdeError) -> Response {
    log_serialize_failure(&err);
    IntoResponse::<RestXml>::into_response(RuntimeError::Serialization(crate::Error::new(err)))
}
