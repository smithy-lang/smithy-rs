/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerializableStruct, ShapeDeserializer};
use aws_smithy_schema::OperationSchema;

use crate::response::Response;
use crate::schema::DeserializeError;

use super::request::{check_accept, rpc_request_deserializer};
use super::response::serialize_rpc_operation_response;
use super::{OperationState, RequestBodyHandling, ServerRequest};

/// Determines which RPC operations advertise a response entity to the `Accept` gate.
#[derive(Debug, Clone, Copy)]
pub(crate) enum RpcAccept {
    Always,
    ModeledOutput,
}

/// The body codec and HTTP framing shared by one RPC protocol implementation.
///
/// Error discrimination and rejection responses deliberately remain on the concrete protocol:
/// those are wire policy, not codec mechanics.
#[derive(Debug)]
pub(crate) struct RpcProtocol<C> {
    codec: C,
    content_type: &'static str,
    empty_response_content_type: Option<&'static str>,
    accept: RpcAccept,
}

impl<C> RpcProtocol<C> {
    pub(crate) fn new(
        codec: C,
        content_type: &'static str,
        empty_response_content_type: Option<&'static str>,
        accept: RpcAccept,
    ) -> Self {
        Self {
            codec,
            content_type,
            empty_response_content_type,
            accept,
        }
    }

    pub(crate) fn codec(&self) -> &C {
        &self.codec
    }

    pub(crate) fn compile_operation(&self, schema: &'static OperationSchema<'static>) -> RpcOperationState {
        RpcOperationState::compile(schema, self.accept)
    }
}

impl<C: Codec> RpcProtocol<C> {
    pub(crate) fn deserialize_request<'a>(
        &'a self,
        state: &RpcOperationState,
        input: &'a aws_smithy_schema::Schema<'a>,
        request: &'a ServerRequest,
    ) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError> {
        if state.check_accept {
            check_accept(&request.headers, self.content_type)?;
        }
        rpc_request_deserializer(&self.codec, self.content_type, input, request)
    }

    pub(crate) fn serialize_response(
        &self,
        operation: &OperationSchema<'_>,
        output: &dyn SerializableStruct,
    ) -> Result<Response, aws_smithy_schema::serde::SerdeError> {
        serialize_rpc_operation_response(
            &self.codec,
            operation,
            output,
            self.content_type,
            self.empty_response_content_type,
        )
    }
}

pub(crate) trait RpcProtocolProvider {
    type RpcCodec: Codec + Send + Sync + std::fmt::Debug + 'static;

    fn rpc_protocol(&self) -> &RpcProtocol<Self::RpcCodec>;
}

#[derive(Debug)]
pub struct RpcOperationState {
    request_body: RequestBodyHandling,
    check_accept: bool,
}

impl RpcOperationState {
    fn compile(schema: &'static OperationSchema<'static>, accept: RpcAccept) -> Self {
        let request_body = if schema.input().members().iter().any(|member| member.streaming()) {
            RequestBodyHandling::Streaming
        } else if schema.input().members().is_empty() {
            RequestBodyHandling::Unused
        } else {
            RequestBodyHandling::Collected
        };
        let check_accept = match accept {
            RpcAccept::Always => true,
            RpcAccept::ModeledOutput => schema.output().original_name().is_some(),
        };
        Self {
            request_body,
            check_accept,
        }
    }
}

impl OperationState for RpcOperationState {
    fn request_body(&self) -> RequestBodyHandling {
        self.request_body
    }
}
