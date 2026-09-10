/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{OperationSchema, Schema};

use crate::response::Response;
use crate::schema::response_bindings::{has_output_body_members, has_response_bound_members};
use crate::schema::DeserializeError;

use super::request::{
    enforce_content_type, enforce_expected_accept, expected_request_content_type, expected_response_content_type,
    ExpectedContentType,
};
use super::response::{serialize_compiled_rest_operation_response, CompiledRestResponseFacts};
use super::{OperationState, RequestBodyHandling, ServerRequest};

#[derive(Debug)]
pub(crate) struct RestProtocol<C> {
    codec: C,
    content_type: &'static str,
}

impl<C> RestProtocol<C> {
    pub(crate) fn new(codec: C, content_type: &'static str) -> Self {
        Self { codec, content_type }
    }

    pub(crate) fn codec(&self) -> &C {
        &self.codec
    }

    pub(crate) fn compile_operation(&self, schema: &'static OperationSchema<'static>) -> RestOperationState {
        RestOperationState::compile(schema, self.content_type)
    }
}

impl<C: Codec> RestProtocol<C> {
    pub(crate) fn deserialize_request<'a>(
        &'a self,
        state: &'a RestOperationState,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        enforce_expected_accept(&request.headers, state.expected_response_type.as_ref())?;
        enforce_content_type(&state.expected_request_content_type, &request.headers, &request.body)?;
        Ok(Box::new(crate::schema::request_bindings::RestRequestDeserializer::new(
            &self.codec,
            &request.uri,
            &request.headers,
            &request.body,
            state.uri_template,
        )))
    }

    pub(crate) fn serialize_response(
        &self,
        operation: &OperationSchema<'_>,
        state: &RestOperationState,
        output: &dyn SerializableStruct,
    ) -> Result<Response, aws_smithy_schema::serde::SerdeError> {
        serialize_compiled_rest_operation_response(
            &self.codec,
            operation,
            output,
            self.content_type,
            None,
            CompiledRestResponseFacts {
                output_has_body: state.output_has_body,
                has_response_bindings: state.has_response_bindings,
                default_status: state.default_status,
            },
        )
    }
}

pub(crate) trait RestProtocolProvider {
    type RestCodec: Codec + Send + Sync + std::fmt::Debug + 'static;
    fn rest_protocol(&self) -> &RestProtocol<Self::RestCodec>;
}

#[derive(Debug)]
pub struct RestOperationState {
    request_body: RequestBodyHandling,
    request_payload: Option<&'static Schema<'static>>,
    response_payload: Option<&'static Schema<'static>>,
    expected_request_content_type: ExpectedContentType,
    expected_response_type: Option<mime::Mime>,
    output_has_body: bool,
    has_response_bindings: bool,
    uri_template: Option<&'static str>,
    default_status: u16,
}

impl RestOperationState {
    fn compile(operation: &'static OperationSchema<'static>, content_type: &'static str) -> Self {
        let input = operation.input();
        let output = operation.output();
        let request_payload = input.members().iter().copied().find(|m| m.http_payload().is_some());
        let response_payload = output.members().iter().copied().find(|m| m.http_payload().is_some());
        let has_body_binding = input.members().iter().any(|m| {
            m.http_payload().is_some()
                || (m.http_header().is_none()
                    && m.http_query().is_none()
                    && m.http_label().is_none()
                    && m.http_prefix_headers().is_none()
                    && m.http_query_params().is_none())
        });
        let request_body = if input.members().iter().any(|m| m.streaming()) {
            RequestBodyHandling::Streaming
        } else if has_body_binding {
            RequestBodyHandling::Collected
        } else {
            RequestBodyHandling::Unused
        };
        let http = operation.schema().http();
        Self {
            request_body,
            request_payload,
            response_payload,
            expected_request_content_type: expected_request_content_type(input, content_type),
            expected_response_type: expected_response_content_type(output, content_type),
            output_has_body: has_output_body_members(output, true),
            has_response_bindings: has_response_bound_members(output),
            uri_template: http.map(|http| http.uri()),
            default_status: http.map(|http| http.code()).unwrap_or(200),
        }
    }

    pub fn request_payload(&self) -> Option<&'static Schema<'static>> {
        self.request_payload
    }
    pub fn response_payload(&self) -> Option<&'static Schema<'static>> {
        self.response_payload
    }
}

impl OperationState for RestOperationState {
    fn request_body(&self) -> RequestBodyHandling {
        self.request_body
    }
}
