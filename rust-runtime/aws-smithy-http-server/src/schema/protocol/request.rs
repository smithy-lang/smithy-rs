/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Request-side helpers shared by the protocol implementations: `Accept` and `Content-Type`
//! gating and the construction of request deserializers.

use aws_smithy_runtime_api::http::Headers;
use aws_smithy_schema::codec::Codec;
use aws_smithy_schema::serde::ShapeDeserializer;
use aws_smithy_schema::{Schema, ShapeType};

use crate::rejection::MissingContentTypeReason;
use crate::schema::request_bindings::EmptyStructDeserializer;
use crate::schema::DeserializeError;

use super::ServerRequest;

/// What the `Content-Type` header must look like for a request with this input.
#[derive(Debug)]
pub(super) enum ExpectedContentType {
    /// Do not look at the header.
    Skip,
    /// The header must be absent.
    Absent,
    /// The header must carry this media type when the body is not empty.
    Expect(mime::Mime),
}

/// The `Content-Type` rules for a REST request with this input.
///
/// A `@httpPayload` member fixes the expected type: `@mediaType` when present, `text/plain` for
/// strings, the codec's type for structures and documents, and no check for a blob without a media
/// type. An input with no members must have no `Content-Type` at all, unless the input was
/// modeled by the user (the schema then carries an original name) in which case the header is
/// ignored. Otherwise the codec's type is expected when any member is bound to the body.
pub(super) fn expected_request_content_type(
    input: &Schema<'_>,
    codec_content_type: &'static str,
) -> ExpectedContentType {
    if let Some(payload) = input.members().iter().find(|m| m.http_payload().is_some()) {
        let media_type = payload.media_type().map(|m| m.value());
        return match (payload.shape_type(), media_type) {
            (ShapeType::Blob, None) => ExpectedContentType::Skip,
            (ShapeType::Blob, Some(media)) => {
                ExpectedContentType::Expect(media.parse().expect("Smithy mediaType must be a MIME type"))
            }
            (ShapeType::String, media) => ExpectedContentType::Expect(
                media
                    .unwrap_or("text/plain")
                    .parse()
                    .expect("expected MIME type must be valid"),
            ),
            _ => ExpectedContentType::Expect(codec_content_type.parse().expect("protocol content type must be valid")),
        };
    }
    if input.members().is_empty() {
        return if input.original_name().is_none() {
            ExpectedContentType::Absent
        } else {
            ExpectedContentType::Skip
        };
    }
    if input.members().iter().any(|m| is_body_member(m)) {
        ExpectedContentType::Expect(codec_content_type.parse().expect("protocol content type must be valid"))
    } else {
        ExpectedContentType::Skip
    }
}

/// `true` when `member` travels in the body rather than in the URI or headers.
fn is_body_member(member: &Schema<'_>) -> bool {
    member.http_header().is_none()
        && member.http_query().is_none()
        && member.http_label().is_none()
        && member.http_prefix_headers().is_none()
        && member.http_query_params().is_none()
}

fn check_content_type(headers: &Headers, expected: Option<&str>) -> Result<(), DeserializeError> {
    let actual = headers.get(http::header::CONTENT_TYPE.as_str());
    let parse = |s: &str| {
        s.parse::<mime::Mime>()
            .map_err(|err| DeserializeError::from(MissingContentTypeReason::MimeParseError(err)))
    };
    let unexpected = |expected: Option<&str>, found_mime: Option<mime::Mime>| {
        DeserializeError::from(MissingContentTypeReason::UnexpectedMimeType {
            expected_mime: expected.and_then(|e| e.parse().ok()),
            found_mime,
        })
    };
    match (actual, expected) {
        (None, None) => Ok(()),
        (None, Some(expected)) => Err(unexpected(Some(expected), None)),
        (Some(actual), None) => Err(unexpected(None, Some(parse(actual)?))),
        (Some(actual), Some(expected)) => {
            let found = parse(actual)?;
            if expected != found.essence_str() {
                Err(unexpected(Some(expected), Some(found)))
            } else {
                Ok(())
            }
        }
    }
}

/// Enforces `expected`. An empty body is accepted without a header: the header is only checked
/// when there are bytes to parse.
pub(super) fn enforce_content_type(
    headers: &Headers,
    expected: &ExpectedContentType,
    body: &[u8],
) -> Result<(), DeserializeError> {
    match expected {
        ExpectedContentType::Skip => Ok(()),
        ExpectedContentType::Absent => check_content_type(headers, None),
        ExpectedContentType::Expect(_) if body.is_empty() => Ok(()),
        ExpectedContentType::Expect(content_type) => check_content_type(headers, Some(content_type.essence_str())),
    }
}

// ============================================================================
// Accept-header gating
// ============================================================================

/// `true` when the request's `Accept` header can accept `content_type`.
///
/// A missing header accepts everything; each header value is split on commas, `;q=` parameters
/// are dropped, and `type/subtype`, `type/*` and `*/*` all match.
fn accept_permits(headers: &Headers, content_type: &mime::Mime) -> bool {
    if !headers.contains_key(http::header::ACCEPT.as_str()) {
        return true;
    }
    headers
        .get_all(http::header::ACCEPT.as_str())
        .flat_map(|value| value.split(',').map(|typ| typ.split(';').next().unwrap().trim()))
        .filter_map(|h| h.parse::<mime::Mime>().ok())
        .any(|mim| {
            let typ = content_type.type_();
            let subtype = content_type.subtype();
            match (mim.type_(), mim.subtype()) {
                (t, s) if t == typ && s == subtype => true,
                (t, mime::STAR) if t == typ => true,
                (mime::STAR, mime::STAR) => true,
                _ => false,
            }
        })
}

/// Rejects the request when its `Accept` header cannot accept `expected`.
///
/// Runs before any deserialization. The RPC protocols call this with their fixed content type;
/// the REST protocols compute the expectation from the output schema via [`check_rest_accept`].
pub(super) fn check_accept(headers: &Headers, expected: &str) -> Result<(), DeserializeError> {
    let Ok(mime) = expected.parse::<mime::Mime>() else {
        // An unparseable expectation can only come from a malformed model; skip the check.
        tracing::debug!(
            content_type = expected,
            "expected response content type is not a valid mime type"
        );
        return Ok(());
    };
    if accept_permits(headers, &mime) {
        Ok(())
    } else {
        Err(DeserializeError::NotAcceptable)
    }
}

pub(super) fn enforce_expected_accept(
    headers: &Headers,
    expected: Option<&mime::Mime>,
) -> Result<(), DeserializeError> {
    match expected {
        Some(expected) if !accept_permits(headers, expected) => Err(DeserializeError::NotAcceptable),
        _ => Ok(()),
    }
}

/// The `Content-Type` a REST response for `output` will carry, if any.
///
/// Runtime mirror of `HttpBindingIndex.determineResponseContentType`, which decides at codegen
/// time whether an operation gets an `Accept` gate and against which type: a `@httpPayload`
/// member fixes the type (codec type for aggregates, `@mediaType` when present, otherwise
/// `application/octet-stream` for blobs and `text/plain` for strings); else the codec's type when
/// any member is bound to the body; else no gate at all.
pub(super) fn expected_response_content_type(
    output: &Schema<'_>,
    codec_content_type: &'static str,
) -> Option<mime::Mime> {
    if let Some(payload) = output.members().iter().find(|m| m.http_payload().is_some()) {
        return match payload.shape_type() {
            ShapeType::Structure | ShapeType::Document | ShapeType::Union | ShapeType::List | ShapeType::Map => {
                Some(codec_content_type.parse().expect("protocol content type must be valid"))
            }
            _ if payload.media_type().is_some() => payload
                .media_type()
                .map(|m| m.value().parse().expect("Smithy mediaType must be a MIME type")),
            // An untyped blob payload accepts every response media type. The response defaults
            // to `application/octet-stream`, but that default is not an Accept requirement.
            ShapeType::Blob => None,
            ShapeType::String => Some(mime::TEXT_PLAIN),
            _ => None,
        };
    }
    if output.members().iter().any(|m| is_body_member(m)) {
        Some(codec_content_type.parse().expect("protocol content type must be valid"))
    } else {
        None
    }
}

/// Builds the request deserializer for an RPC protocol.
///
/// The body is parsed only when it is non-empty and the input has members: an input without
/// members ignores the body entirely, and an empty body reads as a structure with no members
/// present so that `@required` is enforced by the builder.
///
/// The `Accept` gate is the caller's: awsJson checks its fixed content type unconditionally,
/// rpcv2Cbor only for operations with user-modeled output.
pub(super) fn rpc_request_deserializer<'a, C>(
    codec: &'a C,
    codec_content_type: &'static str,
    input: &Schema<'_>,
    request: &'a ServerRequest,
) -> Result<Box<dyn ShapeDeserializer + 'a>, DeserializeError>
where
    C: Codec,
{
    if request.body.is_empty() || input.members().is_empty() {
        return Ok(Box::new(EmptyStructDeserializer));
    }
    check_content_type(&request.headers, Some(codec_content_type))?;
    Ok(Box::new(codec.create_deserializer(&request.body)))
}
