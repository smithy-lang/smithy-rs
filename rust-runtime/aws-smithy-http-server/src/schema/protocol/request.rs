/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use std::borrow::Cow;

use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::{Schema, ShapeType};
use bytes::Bytes;

use crate::deserialize::DeserializeError;
use crate::rejection::MissingContentTypeReason;
use crate::schema::request_bindings::{EmptyStructDeserializer, RestRequestDeserializer};

// ============================================================================
// Shared request-side helpers
// ============================================================================

/// What the request's `Content-Type` header must look like for an input
/// schema, mirroring the legacy generated checks.
enum ExpectedContentType<'s> {
    /// No check at all (all members bound to non-body locations, or a blob
    /// `@httpPayload` without `@mediaType` — the legacy generator skips the
    /// check for those).
    Skip,
    /// The header must be absent (`serverContentTypeCheckNoModeledInput`
    /// protocols, operations without modeled input). Checked even on an
    /// empty body.
    Absent,
    /// The header must match — but only when the request body is non-empty
    /// (the legacy `if !bytes.is_empty()` gate; see smithy-lang/smithy#2327).
    Expect(Cow<'s, str>),
}

/// Computes the `Content-Type` expectation for this input schema: the payload
/// member's content type when `@httpPayload` is modeled, the protocol's codec
/// content type when unbound (body) members exist, absence for
/// no-modeled-input operations on protocols that demand it.
fn expected_request_content_type<'s>(
    schema: &'s Schema<'s>,
    codec_content_type: &'static str,
    check_absent_when_no_input: bool,
) -> ExpectedContentType<'s> {
    if let Some(payload) = schema.members().iter().find(|m| m.http_payload().is_some()) {
        let media_type = payload.media_type().map(|m| Cow::Borrowed(m.value()));
        return match (payload.shape_type(), media_type) {
            // Legacy skips the check for blob payloads without @mediaType.
            (ShapeType::Blob, None) => ExpectedContentType::Skip,
            (ShapeType::Blob, Some(media)) => ExpectedContentType::Expect(media),
            (ShapeType::String, media) => ExpectedContentType::Expect(media.unwrap_or(Cow::Borrowed("text/plain"))),
            _ => ExpectedContentType::Expect(Cow::Borrowed(codec_content_type)),
        };
    }
    if schema.members().is_empty() {
        // `original_name` is transcribed from the synthetic input trait's
        // original id, which exists exactly when the operation had
        // user-modeled input — the legacy `hadUserModeledOperationInput`
        // signal. A user-modeled EMPTY input struct therefore skips the
        // absence check (RestJsonEmptyInputAndEmptyOutput accepts a
        // `Content-Type` header).
        return if check_absent_when_no_input && schema.original_name().is_none() {
            ExpectedContentType::Absent
        } else {
            ExpectedContentType::Skip
        };
    }
    let has_unbound_members = schema.members().iter().any(|m| {
        m.http_header().is_none()
            && m.http_query().is_none()
            && m.http_label().is_none()
            && m.http_prefix_headers().is_none()
            && m.http_query_params().is_none()
    });
    if has_unbound_members {
        ExpectedContentType::Expect(Cow::Borrowed(codec_content_type))
    } else {
        ExpectedContentType::Skip
    }
}

/// Checks the request `Content-Type` header against the expected value,
/// mirroring [`super::content_type_header_classifier_smithy`] for
/// `http::HeaderMap` and non-`'static` expected values.
#[allow(clippy::result_large_err)]
fn check_content_type(headers: &http::HeaderMap, expected: Option<&str>) -> Result<(), MissingContentTypeReason> {
    let actual = match headers.get(http::header::CONTENT_TYPE) {
        Some(value) => Some(
            value
                .to_str()
                .map_err(|_| MissingContentTypeReason::UnexpectedMimeType {
                    expected_mime: expected.and_then(|e| e.parse().ok()),
                    found_mime: None,
                })?,
        ),
        None => None,
    };
    let parse = |s: &str| {
        s.parse::<mime::Mime>()
            .map_err(MissingContentTypeReason::MimeParseError)
    };
    match (actual, expected) {
        (None, None) => Ok(()),
        (None, Some(expected)) => Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: expected.parse().ok(),
            found_mime: None,
        }),
        (Some(actual), None) => Err(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: None,
            found_mime: Some(parse(actual)?),
        }),
        (Some(actual), Some(expected)) => {
            let found = parse(actual)?;
            if expected != found.essence_str() {
                Err(MissingContentTypeReason::UnexpectedMimeType {
                    expected_mime: expected.parse().ok(),
                    found_mime: Some(found),
                })
            } else {
                Ok(())
            }
        }
    }
}

/// Runs the content-type expectation against the request, mirroring the
/// legacy gating: `Expect` only applies to non-empty bodies; `Absent` is
/// checked unconditionally.
#[allow(clippy::result_large_err)]
fn enforce_content_type(
    expected: ExpectedContentType<'_>,
    parts: &http::request::Parts,
    body: &[u8],
) -> Result<(), MissingContentTypeReason> {
    match expected {
        ExpectedContentType::Skip => Ok(()),
        ExpectedContentType::Absent => check_content_type(&parts.headers, None),
        ExpectedContentType::Expect(content_type) => {
            if body.is_empty() {
                Ok(())
            } else {
                check_content_type(&parts.headers, Some(content_type.as_ref()))
            }
        }
    }
}

#[allow(clippy::result_large_err)]
fn enforce_request_content_type(
    expected: ExpectedContentType<'_>,
    request: &http::Request<Bytes>,
) -> Result<(), MissingContentTypeReason> {
    match expected {
        ExpectedContentType::Skip => Ok(()),
        ExpectedContentType::Absent => check_content_type(request.headers(), None),
        ExpectedContentType::Expect(content_type) => {
            if request.body().is_empty() {
                Ok(())
            } else {
                check_content_type(request.headers(), Some(content_type.as_ref()))
            }
        }
    }
}

/// The shared REST request path: content-type validation, then the composite
/// binding deserializer driving the generated walker.
pub(super) fn deserialize_rest_request<Out, F, C, R>(
    codec: &'static C,
    codec_content_type: &'static str,
    check_absent_when_no_input: bool,
    schema: &Schema<'_>,
    parts: &http::request::Parts,
    body: &[u8],
    f: F,
) -> Result<Out, R>
where
    F: FnOnce(&mut dyn ShapeDeserializer) -> Result<Out, DeserializeError>,
    C: Codec,
    R: From<DeserializeError> + From<MissingContentTypeReason>,
{
    let expected = expected_request_content_type(schema, codec_content_type, check_absent_when_no_input);
    enforce_content_type(expected, parts, body).map_err(R::from)?;
    let mut deserializer = RestRequestDeserializer::new(codec, parts, body);
    f(&mut deserializer).map_err(R::from)
}

/// The shared REST request path for dynamic dispatch: content-type validation,
/// then a boxed composite binding deserializer.
pub(super) fn rest_request_deserializer<'a, C, R>(
    codec: &'static C,
    codec_content_type: &'static str,
    check_absent_when_no_input: bool,
    schema: &Schema<'_>,
    request: &'a http::Request<Bytes>,
) -> Result<Box<dyn ShapeDeserializer + 'a>, R>
where
    C: Codec,
    R: From<MissingContentTypeReason>,
{
    let expected = expected_request_content_type(schema, codec_content_type, check_absent_when_no_input);
    enforce_request_content_type(expected, request).map_err(R::from)?;
    Ok(Box::new(RestRequestDeserializer::from_request(codec, request)))
}

/// The shared RPC request path: content-type validation (non-empty bodies
/// only, per the legacy gate), then body-only deserialization through the
/// codec (an empty body reads as a structure with no members present —
/// `@required` enforcement stays in `build()`).
pub(super) fn deserialize_rpc_request<Out, F, C, R>(
    codec: &'static C,
    codec_content_type: &'static str,
    schema: &Schema<'_>,
    parts: &http::request::Parts,
    body: &[u8],
    f: F,
) -> Result<Out, R>
where
    F: FnOnce(&mut dyn ShapeDeserializer) -> Result<Out, DeserializeError>,
    C: Codec,
    R: From<DeserializeError> + From<MissingContentTypeReason>,
{
    // An input schema with no members mirrors the legacy `parser == null`
    // case: the body is never parsed OR content-type checked, whatever it
    // contains (rpcv2Cbor's NoInputOutput tests send an empty CBOR map).
    // An empty body reads as a structure with no members present —
    // `@required` enforcement stays in `build()`.
    //
    // Event-stream inputs skip the Content-Type check: the HTTP header carries
    // the event-stream content type, and the "body" handed here is the
    // initial-request frame's codec payload.
    if !body.is_empty() && !schema.members().is_empty() {
        check_content_type(&parts.headers, Some(codec_content_type)).map_err(R::from)?;
        let mut deserializer = codec.create_deserializer(body);
        f(&mut deserializer).map_err(R::from)
    } else {
        f(&mut EmptyStructDeserializer).map_err(R::from)
    }
}

/// The shared RPC request path for dynamic dispatch: content-type validation,
/// then a boxed body deserializer.
pub(super) fn rpc_request_deserializer<'a, C, R>(
    codec: &'static C,
    codec_content_type: &'static str,
    schema: &Schema<'_>,
    request: &'a http::Request<Bytes>,
) -> Result<Box<dyn ShapeDeserializer + 'a>, R>
where
    C: Codec,
    R: From<MissingContentTypeReason>,
{
    if !request.body().is_empty() && !schema.members().is_empty() {
        check_content_type(request.headers(), Some(codec_content_type)).map_err(R::from)?;
        Ok(Box::new(codec.create_deserializer(request.body().as_ref())))
    } else {
        Ok(Box::new(EmptyStructDeserializer))
    }
}
