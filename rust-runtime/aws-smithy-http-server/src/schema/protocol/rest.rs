/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::{OperationSchema, Schema};

use crate::response::Response;
use crate::schema::response_bindings::CompiledResponsePlan;
use crate::schema::DeserializeError;

use super::request::{
    enforce_content_type, enforce_expected_accept, expected_request_content_type, expected_response_content_type,
    is_body_member, ExpectedContentType,
};
use super::response::serialize_compiled_rest_operation_response;
use super::{OperationState, ServerRequest};

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
        enforce_content_type(&request.headers, &state.expected_request_content_type, &request.body)?;
        Ok(Box::new(crate::schema::request_bindings::RestRequestDeserializer::new(
            &self.codec,
            &request.uri,
            &request.headers,
            &request.body,
            state,
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
            &state.response,
            state.default_status,
        )
    }
}

pub(crate) trait RestProtocolProvider {
    type RestCodec: Codec + Send + Sync + std::fmt::Debug + 'static;
    fn rest_protocol(&self) -> &RestProtocol<Self::RestCodec>;
}

#[derive(Debug)]
pub struct RestOperationState {
    reads_body: bool,
    request_payload: Option<&'static Schema<'static>>,
    response_payload: Option<&'static Schema<'static>>,
    expected_request_content_type: ExpectedContentType,
    expected_response_type: Option<mime::Mime>,
    response: CompiledResponsePlan,
    uri_template: Option<&'static str>,
    has_labels: bool,
    needs_query: bool,
    has_unbound_members: bool,
    default_status: u16,
}

impl RestOperationState {
    fn compile(operation: &'static OperationSchema<'static>, content_type: &'static str) -> Self {
        let input = operation.input();
        let output = operation.output();
        let request_payload = input.members().iter().copied().find(|m| m.http_payload().is_some());
        let response_payload = output.members().iter().copied().find(|m| m.http_payload().is_some());
        let has_unbound_members = input
            .members()
            .iter()
            .any(|m| m.http_payload().is_none() && is_body_member(m));
        let http = operation.schema().http();
        Self {
            // The legacy REST deserializers never touch the body when nothing is bound to it.
            reads_body: request_payload.is_some() || has_unbound_members,
            request_payload,
            response_payload,
            expected_request_content_type: expected_request_content_type(input, content_type),
            expected_response_type: expected_response_content_type(output, content_type),
            response: CompiledResponsePlan::operation_output(output),
            uri_template: http.map(|http| http.uri()),
            has_labels: input.members().iter().any(|member| member.http_label().is_some()),
            needs_query: input
                .members()
                .iter()
                .any(|member| member.http_query().is_some() || member.http_query_params().is_some()),
            has_unbound_members,
            default_status: http.map(|http| http.code()).unwrap_or(200),
        }
    }

    pub(crate) fn reads_body(&self) -> bool {
        self.reads_body
    }

    pub fn request_payload(&self) -> Option<&'static Schema<'static>> {
        self.request_payload
    }
    pub fn response_payload(&self) -> Option<&'static Schema<'static>> {
        self.response_payload
    }

    pub(crate) fn uri_template(&self) -> Option<&'static str> {
        self.uri_template
    }
    pub(crate) fn has_labels(&self) -> bool {
        self.has_labels
    }
    pub(crate) fn needs_query(&self) -> bool {
        self.needs_query
    }
    pub(crate) fn has_unbound_members(&self) -> bool {
        self.has_unbound_members
    }
}

impl OperationState for RestOperationState {}
