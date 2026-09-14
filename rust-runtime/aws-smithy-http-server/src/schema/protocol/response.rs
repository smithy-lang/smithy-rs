/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Response-side helpers shared by the protocol implementations.

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct};
use aws_smithy_schema::traits::HttpTrait;
use aws_smithy_schema::Schema;

use crate::body::BoxBody;
use crate::extension::{ModeledErrorExtension, RuntimeErrorExtension};
use crate::response::Response;
use crate::schema::response_bindings::{serialize_response_parts, ResponseParts, ResponseValueKind};

pub use crate::schema::response_bindings::ResponseBindings;

/// The success status: a captured `@httpResponseCode`, else the output's `@http` code, else `200`.
pub(crate) fn resolve_status(captured: Option<u16>, http: Option<&HttpTrait<'_>>) -> u16 {
    captured.or_else(|| http.map(HttpTrait::code)).unwrap_or(200)
}

fn response_head(split: &ResponseParts, status: u16, content_type: Option<&str>) -> http::response::Builder {
    let mut builder = http::Response::builder()
        .status(http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR));
    if let Some(content_type) = content_type {
        builder = builder.header(http::header::CONTENT_TYPE, content_type);
    }
    for (name, value) in &split.headers {
        builder = builder.header(name, value);
    }
    builder
}

fn build_failure(err: http::Error) -> SerdeError {
    SerdeError::custom(format!("failed to build response: {err}"))
}

/// Assembles an in-memory HTTP response from split parts. `Content-Length` is always set.
pub(super) fn assemble_response(
    split: ResponseParts,
    status: u16,
    content_type: Option<&str>,
) -> Result<Response, SerdeError> {
    response_head(&split, status, content_type)
        .header(http::header::CONTENT_LENGTH, split.body.len())
        .body(crate::body::to_boxed(split.body))
        .map_err(build_failure)
}

/// Assembles the head of a streaming HTTP response around `body`. The split body is ignored and
/// no `Content-Length` is set.
pub(super) fn assemble_streaming_response(
    split: ResponseParts,
    status: u16,
    content_type: Option<&str>,
    body: BoxBody,
) -> Result<Response, SerdeError> {
    response_head(&split, status, content_type)
        .body(body)
        .map_err(build_failure)
}

/// Serializes a modeled error; `bindings` says whether its HTTP-bound members leave the body.
pub fn serialize_modeled_error_response<C: Codec>(
    codec: &C,
    schema: &Schema<'_>,
    error: &dyn SerializableStruct,
    status: u16,
    bindings: ResponseBindings,
    codec_content_type: &'static str,
) -> Result<Response, SerdeError> {
    let parts = serialize_response_parts(codec, schema, error, bindings, ResponseValueKind::ModeledError)?;
    assemble_response(parts, status, Some(codec_content_type))
}

/// Records the error's shape name in the response extensions for instrumentation.
pub fn stamp_error_extension(mut response: Response, error_name: &str) -> Response {
    response
        .extensions_mut()
        .insert(ModeledErrorExtension::new(aws_smithy_schema::intern_header_name(
            error_name,
        )));
    response
}

/// Marks a modeled validation response with the same `RuntimeErrorExtension` that
/// `RuntimeError::Validation` responses carry, so instrumentation sees one shape.
pub fn stamp_validation_extension(mut response: Response) -> Response {
    response
        .extensions_mut()
        .insert(RuntimeErrorExtension::new("ValidationException".to_string()));
    response
}

pub fn log_serialize_failure(err: &SerdeError) {
    tracing::error!(error = %err, "failed to serialize response");
}
