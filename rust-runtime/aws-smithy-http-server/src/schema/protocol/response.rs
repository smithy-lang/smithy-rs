/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::{SerdeError, SerializableStruct, ShapeSerializer};
use aws_smithy_schema::Schema;

use crate::body::BoxBody;
use crate::extension::ModeledErrorExtension;
use crate::schema::response_bindings::{
    resolve_status, serialize_response_parts, BodyKind, ResponseParts, ResponseValueKind,
};

/// Sized adapter so a `&E` with `E: SerializableStruct + ?Sized` (e.g.
/// `&dyn HttpModeledError`) can be passed where `&dyn SerializableStruct` is
/// required — unsized-to-`dyn` coercion needs a sized source.
pub(super) struct AsSerializable<'a, E: ?Sized>(pub(super) &'a E);

impl<E: SerializableStruct + ?Sized> SerializableStruct for AsSerializable<'_, E> {
    fn serialize_members(&self, serializer: &mut dyn ShapeSerializer) -> Result<(), SerdeError> {
        self.0.serialize_members(serializer)
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum ResponseBindingMode {
    Rest,
    BodyOnly,
}

impl ResponseBindingMode {
    fn apply_response_bindings(self) -> bool {
        matches!(self, Self::Rest)
    }
}

// ============================================================================
// Shared response assembly
// ============================================================================

/// Assembles a response from [`ResponseParts`]: status, content type per
/// [`BodyKind`], captured binding headers, content-length.
///
/// Mirrors the legacy generated `ser_*_http_response` functions, which stamp
/// content-type, binding headers, and content-length via
/// `set_response_header_if_absent` (the headers cannot already be present on
/// a fresh builder, so plain insertion is equivalent).
pub(super) fn assemble_response(
    split: ResponseParts,
    status: u16,
    codec_content_type: &'static str,
    empty_content_type: Option<&'static str>,
) -> Result<http::Response<BoxBody>, SerdeError> {
    let mut builder = http::Response::builder()
        .status(http::StatusCode::from_u16(status).unwrap_or(http::StatusCode::INTERNAL_SERVER_ERROR));
    let content_type: Option<Cow<'_, str>> = match &split.kind {
        BodyKind::Codec => Some(Cow::Borrowed(codec_content_type)),
        BodyKind::Raw { content_type } => Some(Cow::Borrowed(content_type.as_str())),
        BodyKind::Empty => empty_content_type.map(Cow::Borrowed),
    };
    if let Some(content_type) = content_type {
        builder = builder.header(http::header::CONTENT_TYPE, content_type.as_ref());
    }
    for (name, value) in split.headers {
        builder = builder.header(name, value);
    }
    builder = builder.header(http::header::CONTENT_LENGTH, split.body.len());
    builder
        .body(crate::body::to_boxed(split.body))
        .map_err(|err| SerdeError::custom(format!("failed to build response: {err}")))
}

pub(super) fn serialize_operation_response<C: Codec>(
    codec: &'static C,
    schema: &Schema<'_>,
    output: &dyn SerializableStruct,
    response_binding_mode: ResponseBindingMode,
    codec_content_type: &'static str,
    empty_content_type: Option<&'static str>,
) -> Result<http::Response<BoxBody>, SerdeError> {
    let parts = serialize_response_parts(
        codec,
        schema,
        output,
        response_binding_mode.apply_response_bindings(),
        ResponseValueKind::OperationOutput,
    )?;
    let status = resolve_status(parts.status, schema);
    assemble_response(parts, status, codec_content_type, empty_content_type)
}

pub(super) fn serialize_modeled_error_response<C: Codec>(
    codec: &'static C,
    schema: &Schema<'_>,
    error: &dyn SerializableStruct,
    status: u16,
    response_binding_mode: ResponseBindingMode,
    codec_content_type: &'static str,
) -> Result<http::Response<BoxBody>, SerdeError> {
    let parts = serialize_response_parts(
        codec,
        schema,
        error,
        response_binding_mode.apply_response_bindings(),
        ResponseValueKind::ModeledError,
    )?;
    assemble_response(parts, status, codec_content_type, None)
}

/// Finishes an error response: inserts the [`ModeledErrorExtension`],
/// preserving the legacy generated `IntoResponse` behavior.
pub(super) fn stamp_error_extension(
    mut response: http::Response<BoxBody>,
    error_name: &str,
) -> http::Response<BoxBody> {
    // `ModeledErrorExtension` requires `&'static str`; generated schemas are
    // `'static` but the `ModeledError::schema` seam erases that lifetime.
    // Interning is deduplicated and bounded by the number of distinct error
    // shape names in the process.
    response
        .extensions_mut()
        .insert(ModeledErrorExtension::new(aws_smithy_schema::intern_header_name(
            error_name,
        )));
    response
}

pub(super) fn log_serialize_failure(err: &SerdeError) {
    tracing::error!(error = %err, "failed to serialize response");
}
