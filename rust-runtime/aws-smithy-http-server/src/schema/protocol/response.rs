/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Response-side helpers shared by the protocol implementations.

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::{OperationSchema, Schema};

use crate::extension::{ModeledErrorExtension, RuntimeErrorExtension};
use crate::response::Response;
use crate::schema::response_bindings::{
    serialize_response_parts, serialize_response_parts_compiled, BodyKind, CompiledResponsePlan, ResponseParts,
    ResponseValueKind,
};

pub(super) use crate::schema::response_bindings::ResponseBindings;

/// The success status: a captured `@httpResponseCode`, else the operation's `@http` code, else `200`.
pub(crate) fn resolve_status(captured: Option<u16>, http: Option<&HttpTrait<'_>>) -> u16 {
    captured.or_else(|| http.map(HttpTrait::code)).unwrap_or(200)
}

/// Assembles the HTTP response from split parts.
///
/// The content type follows the body kind: the codec's type for codec bodies, the payload's own
/// type for raw payloads, and `empty_content_type` for empty bodies. `Content-Length` is always set.
pub(super) fn assemble_response(
    split: ResponseParts,
    status: u16,
    codec_content_type: &'static str,
    empty_content_type: Option<&'static str>,
) -> Result<Response, SerdeError> {
    let mut builder = http::Response::builder()
        .status(http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR));
    let content_type = match &split.kind {
        BodyKind::Codec => Some(codec_content_type),
        BodyKind::Raw { content_type } => Some(content_type.as_str()),
        BodyKind::Empty => empty_content_type,
    };
    if let Some(content_type) = content_type {
        builder = builder.header(http::header::CONTENT_TYPE, content_type);
    }
    for (name, value) in split.headers {
        builder = builder.header(name, value);
    }
    builder = builder.header(http::header::CONTENT_LENGTH, split.body.len());
    builder
        .body(crate::body::to_boxed(split.body))
        .map_err(|err| SerdeError::custom(format!("failed to build response: {err}")))
}

pub(super) fn serialize_compiled_rest_operation_response<C: Codec>(
    codec: &C,
    operation: &OperationSchema<'_>,
    output: &dyn SerializableStruct,
    codec_content_type: &'static str,
    empty_content_type: Option<&'static str>,
    plan: &CompiledResponsePlan,
    default_status: u16,
) -> Result<Response, SerdeError> {
    let parts = serialize_response_parts_compiled(codec, operation.output(), output, plan)?;
    let status = parts.status.unwrap_or(default_status);
    assemble_response(parts, status, codec_content_type, empty_content_type)
}

/// Serializes an RPC operation output entirely through its body codec.
pub(super) fn serialize_rpc_operation_response<C: Codec>(
    codec: &C,
    operation: &OperationSchema<'_>,
    output: &dyn SerializableStruct,
    codec_content_type: &'static str,
    empty_content_type: Option<&'static str>,
) -> Result<Response, SerdeError> {
    let parts = serialize_response_parts(
        codec,
        operation.output(),
        output,
        ResponseBindings::BodyOnly,
        ResponseValueKind::OperationOutput,
    )?;
    let status = resolve_status(parts.status, operation.schema().http());
    assemble_response(parts, status, codec_content_type, empty_content_type)
}

/// Serializes a modeled error; `bindings` says whether its HTTP-bound members leave the body.
pub(super) fn serialize_modeled_error_response<C: Codec>(
    codec: &C,
    schema: &Schema<'_>,
    error: &dyn SerializableStruct,
    status: u16,
    bindings: ResponseBindings,
    codec_content_type: &'static str,
) -> Result<Response, SerdeError> {
    let parts = serialize_response_parts(codec, schema, error, bindings, ResponseValueKind::ModeledError)?;
    assemble_response(parts, status, codec_content_type, None)
}

/// Records the error's shape name in the response extensions for instrumentation.
pub(super) fn stamp_error_extension(mut response: Response, error_name: &str) -> Response {
    response
        .extensions_mut()
        .insert(ModeledErrorExtension::new(aws_smithy_schema::intern_header_name(
            error_name,
        )));
    response
}

/// Marks a modeled validation response with the same `RuntimeErrorExtension` that
/// `RuntimeError::Validation` responses carry, so instrumentation sees one shape.
pub(super) fn stamp_validation_extension(mut response: Response) -> Response {
    response
        .extensions_mut()
        .insert(RuntimeErrorExtension::new("ValidationException".to_string()));
    response
}

pub(super) fn log_serialize_failure(err: &SerdeError) {
    tracing::error!(error = %err, "failed to serialize response");
}
