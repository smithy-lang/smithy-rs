/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The HTTP-binding engine shared by the REST protocols. Everything is derived from the input or
//! output schema on each call; the protocol holds only its codec and its media-type policy.

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{Schema, ShapeType};

use crate::body::BoxBody;
use crate::response::Response;
use crate::schema::request_bindings::RestRequestDeserializer;
use crate::schema::response_bindings::{serialize_response_parts, ResponseBindings, ResponseValueKind};
use crate::schema::DeserializeError;

use super::request::{
    enforce_content_type, enforce_expected_accept, expected_request_content_type, is_body_member, payload_member,
    EVENT_STREAM_CONTENT_TYPE, OCTET_STREAM_CONTENT_TYPE,
};
use super::response::{assemble_response, assemble_streaming_response, resolve_status};
use super::ServerRequest;

/// How a REST protocol labels its responses. These are the rules the legacy generated servers
/// follow, so the schema path stays byte-identical to them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RestPolicy {
    /// The codec's media type: codec-framed bodies and structured payloads.
    pub(crate) codec_content_type: &'static str,
    /// The response media type when nothing in the output schema determines one. restJson1
    /// stamps `application/json` on every such response, restXml stamps nothing.
    pub(crate) default_response_content_type: Option<&'static str>,
    /// The response media type of a non-streaming `@httpPayload` blob without `@mediaType`.
    /// restJson1 sets none, restXml `application/octet-stream`.
    pub(crate) untyped_blob_payload_content_type: Option<&'static str>,
    /// Whether the codec's empty document (`{}` on restJson1) stands in for a missing body: a
    /// user-modeled output with no body members, or an unset structure `@httpPayload`. restXml
    /// sends an empty body in both cases.
    pub(crate) empty_document: bool,
}

#[derive(Debug)]
pub(crate) struct RestProtocol<C> {
    codec: C,
    policy: RestPolicy,
}

impl<C> RestProtocol<C> {
    pub(crate) fn new(codec: C, policy: RestPolicy) -> Self {
        Self { codec, policy }
    }

    pub(crate) fn codec(&self) -> &C {
        &self.codec
    }

    /// The `Content-Type` a response for `output` carries, if any: the runtime mirror of the
    /// legacy `HttpBindingResolver.responseContentType`. It depends on the schema alone, never on
    /// which members are set.
    pub(crate) fn response_content_type<'s>(&self, output: &'s Schema<'s>) -> Option<&'s str> {
        if let Some(payload) = payload_member(output) {
            return match payload.shape_type() {
                ShapeType::Union if payload.streaming() => Some(EVENT_STREAM_CONTENT_TYPE),
                ShapeType::Structure | ShapeType::Union | ShapeType::Document => Some(self.policy.codec_content_type),
                _ if payload.media_type().is_some() => payload.media_type().map(|m| m.value()),
                ShapeType::Blob if payload.streaming() => Some(OCTET_STREAM_CONTENT_TYPE),
                ShapeType::Blob => self.policy.untyped_blob_payload_content_type,
                ShapeType::String => Some("text/plain"),
                _ => Some(self.policy.codec_content_type),
            };
        }
        if output.members().iter().any(|m| is_body_member(m)) {
            Some(self.policy.codec_content_type)
        } else {
            self.policy.default_response_content_type
        }
    }

    /// The legacy REST deserializers never touch the body when nothing is bound to it.
    pub(crate) fn reads_request_body(&self, input: &Schema<'_>) -> bool {
        input.members().iter().any(|m| is_body_member(m) && !m.streaming())
    }

    /// The `Accept` gate is driven by the same media type the response will carry.
    pub(crate) fn check_accept(&self, output: &Schema<'_>, headers: &Headers) -> Result<(), DeserializeError> {
        enforce_expected_accept(headers, self.response_content_type(output))
    }
}

impl<C: Codec> RestProtocol<C> {
    pub(crate) fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        enforce_content_type(
            &request.headers,
            &expected_request_content_type(input, self.policy.codec_content_type),
            &request.body,
        )?;
        Ok(Box::new(RestRequestDeserializer::new(
            &self.codec,
            &request.uri,
            &request.headers,
            &request.body,
        )))
    }

    pub(crate) fn serialize_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<Response, SerdeError> {
        let parts = serialize_response_parts(
            &self.codec,
            output,
            value,
            ResponseBindings::Rest,
            ResponseValueKind::OperationOutput {
                empty_document: self.policy.empty_document,
            },
        )?;
        let status = parts.status.unwrap_or_else(|| resolve_status(None, output.http()));
        assemble_response(parts, status, self.response_content_type(output))
    }

    pub(crate) fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Result<Response, SerdeError> {
        let parts = serialize_response_parts(
            &self.codec,
            output,
            value,
            ResponseBindings::Rest,
            ResponseValueKind::StreamingOutput,
        )?;
        let status = parts.status.unwrap_or_else(|| resolve_status(None, output.http()));
        assemble_streaming_response(parts, status, self.response_content_type(output), body)
    }
}
