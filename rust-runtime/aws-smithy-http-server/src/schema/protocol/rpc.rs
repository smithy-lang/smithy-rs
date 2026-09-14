/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! The body-only engine shared by the RPC protocols: every member travels in the codec body, the
//! `Accept` gate is a fixed policy, and streaming responses are labeled per protocol.

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::Schema;

use crate::body::BoxBody;
use crate::response::Response;
use crate::schema::response_bindings::{serialize_response_parts, ResponseBindings, ResponseValueKind};
use crate::schema::DeserializeError;

use super::request::{
    accept_permits, check_accept, has_streaming_payload, rpc_request_deserializer, EVENT_STREAM_CONTENT_TYPE,
};
use super::response::{assemble_response, assemble_streaming_response, resolve_status};
use super::ServerRequest;

/// Determines which RPC operations advertise a response entity to the `Accept` gate.
#[derive(Debug, Clone, Copy)]
pub enum RpcAccept {
    Always,
    ModeledOutput,
}

/// How an RPC protocol labels event stream responses.
#[derive(Debug, Clone, Copy)]
pub enum RpcStreaming {
    /// The protocol's own content type, on the response and in the `Accept` gate (awsJson).
    CodecContentType,
    /// `application/vnd.amazon.eventstream` on the response; the `Accept` gate takes either that
    /// or the codec's type, which earlier servers accepted by mistake and clients rely on
    /// (rpcv2Cbor).
    EventStreamContentType,
}

/// The body codec and HTTP framing shared by one RPC protocol implementation.
///
/// Error discrimination and rejection responses deliberately remain on the concrete protocol:
/// those are wire policy, not codec mechanics.
#[derive(Debug)]
pub struct RpcProtocol<C> {
    codec: C,
    content_type: &'static str,
    content_type_mime: mime::Mime,
    /// The `Content-Type` of a response whose output was not modeled by the user: awsJson stamps
    /// its type on the empty body, rpcv2Cbor stamps nothing.
    empty_response_content_type: Option<&'static str>,
    accept: RpcAccept,
    streaming: RpcStreaming,
}

impl<C> RpcProtocol<C> {
    pub fn new(
        codec: C,
        content_type: &'static str,
        empty_response_content_type: Option<&'static str>,
        accept: RpcAccept,
        streaming: RpcStreaming,
    ) -> Self {
        Self {
            codec,
            content_type,
            content_type_mime: content_type
                .parse()
                .expect("protocol content type must be a valid MIME type"),
            empty_response_content_type,
            accept,
            streaming,
        }
    }

    pub fn codec(&self) -> &C {
        &self.codec
    }

    /// The legacy RPC deserializers never touch the body of a memberless input.
    pub fn reads_request_body(&self, input: &Schema<'_>) -> bool {
        !input.members().is_empty()
    }

    pub fn check_accept(&self, output: &Schema<'_>, headers: &Headers) -> Result<(), DeserializeError> {
        let gated = match self.accept {
            RpcAccept::Always => true,
            RpcAccept::ModeledOutput => output.original_name().is_some(),
        };
        if !gated {
            return Ok(());
        }
        if has_streaming_payload(output) && matches!(self.streaming, RpcStreaming::EventStreamContentType) {
            let event_stream = EVENT_STREAM_CONTENT_TYPE
                .parse()
                .expect("event stream content type is a valid MIME type");
            return if accept_permits(headers, &event_stream) || accept_permits(headers, &self.content_type_mime) {
                Ok(())
            } else {
                Err(DeserializeError::NotAcceptable)
            };
        }
        check_accept(headers, &self.content_type_mime)
    }

    fn streaming_content_type(&self) -> &'static str {
        match self.streaming {
            RpcStreaming::CodecContentType => self.content_type,
            RpcStreaming::EventStreamContentType => EVENT_STREAM_CONTENT_TYPE,
        }
    }
}

impl<C: Codec> RpcProtocol<C> {
    pub fn deserialize_request<'a>(
        &'a self,
        input: &Schema<'_>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        rpc_request_deserializer(&self.codec, self.content_type, input, request)
    }

    /// A user-modeled output is always a codec document, `{}` or `bf ff` when nothing is set; a
    /// synthetic output is an empty body.
    pub fn serialize_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
    ) -> Result<Response, SerdeError> {
        let parts = serialize_response_parts(
            &self.codec,
            output,
            value,
            ResponseBindings::BodyOnly,
            ResponseValueKind::OperationOutput { empty_document: true },
        )?;
        let status = resolve_status(parts.status, output.http());
        let content_type = if output.original_name().is_some() {
            Some(self.content_type)
        } else {
            self.empty_response_content_type
        };
        assemble_response(parts, status, content_type)
    }

    pub fn serialize_streaming_response(
        &self,
        output: &Schema<'_>,
        value: &dyn SerializableStruct,
        body: BoxBody,
    ) -> Result<Response, SerdeError> {
        let parts = serialize_response_parts(
            &self.codec,
            output,
            value,
            ResponseBindings::BodyOnly,
            ResponseValueKind::StreamingOutput,
        )?;
        let status = resolve_status(None, output.http());
        assemble_streaming_response(parts, status, Some(self.streaming_content_type()), body)
    }
}
