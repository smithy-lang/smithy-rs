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
    for (name, value) in &split.headers {
        builder = builder.header(name, value);
    }
    if let Some(content_type) = content_type {
        if !builder
            .headers_ref()
            .is_some_and(|headers| headers.contains_key(http::header::CONTENT_TYPE))
        {
            builder = builder.header(http::header::CONTENT_TYPE, content_type);
        }
    }
    builder
}

fn build_failure(err: http::Error) -> SerdeError {
    SerdeError::custom(format!("failed to build response: {err}"))
}

/// Validates modeled lengths and emits one canonical `Content-Length`.
///
/// Buffered responses must agree with the serialized body size. Streaming responses may
/// supply a length, but it cannot be checked against the body without collecting the stream.
/// Equal duplicates are coalesced; conflicting or malformed values are rejected.
pub(super) fn normalize_content_length(
    headers: &mut http::HeaderMap,
    buffered_length: Option<u64>,
) -> Result<(), SerdeError> {
    let mut length = buffered_length;
    for value in headers.get_all(http::header::CONTENT_LENGTH) {
        let parsed = value
            .to_str()
            .ok()
            .filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| SerdeError::custom("invalid modeled Content-Length; expected a decimal byte count"))?;
        match length {
            Some(expected) if parsed != expected => {
                return Err(SerdeError::custom(if buffered_length.is_some() {
                    "modeled Content-Length does not match the serialized response body"
                } else {
                    "conflicting modeled Content-Length values"
                }));
            }
            None => length = Some(parsed),
            _ => {}
        }
    }
    if let Some(length) = length {
        headers.insert(http::header::CONTENT_LENGTH, http::HeaderValue::from(length));
    }
    Ok(())
}

/// Assembles a buffered response with one length matching its serialized body.
pub(super) fn assemble_response(
    split: ResponseParts,
    status: u16,
    content_type: Option<&str>,
) -> Result<Response, SerdeError> {
    let body_length = split.body.len() as u64;
    let mut response = response_head(&split, status, content_type)
        .body(crate::body::to_boxed(split.body))
        .map_err(build_failure)?;
    normalize_content_length(response.headers_mut(), Some(body_length))?;
    Ok(response)
}

/// Assembles the head of a streaming HTTP response around `body`. The split body is ignored and
/// no `Content-Length` is inferred. A modeled length is validated and emitted once.
pub(super) fn assemble_streaming_response(
    split: ResponseParts,
    status: u16,
    content_type: Option<&str>,
    body: BoxBody,
) -> Result<Response, SerdeError> {
    let mut response = response_head(&split, status, content_type)
        .body(body)
        .map_err(build_failure)?;
    normalize_content_length(response.headers_mut(), None)?;
    Ok(response)
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
    // Generated validation rejections do not stamp an entity length.
    response.headers_mut().remove(http::header::CONTENT_LENGTH);
    response
        .extensions_mut()
        .insert(RuntimeErrorExtension::new("ValidationException".to_string()));
    response
}

pub fn log_serialize_failure(err: &SerdeError) {
    tracing::error!(error = %err, "failed to serialize response");
}
