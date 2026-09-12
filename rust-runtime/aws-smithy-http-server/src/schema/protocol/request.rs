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

pub(super) use crate::schema::request_bindings::is_body_member;

use super::ServerRequest;

/// The `Content-Type` of event stream requests and responses.
pub(super) const EVENT_STREAM_CONTENT_TYPE: &str = "application/vnd.amazon.eventstream";

/// The response `Content-Type` of a streaming blob without `@mediaType`.
pub(super) const OCTET_STREAM_CONTENT_TYPE: &str = "application/octet-stream";

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

fn parse_mime(value: &str) -> mime::Mime {
    value.parse().expect("modeled media types are valid MIME types")
}

/// The `@httpPayload` member of `schema`, if any.
pub(super) fn payload_member<'s>(schema: &'s Schema<'s>) -> Option<&'s Schema<'s>> {
    schema.members().iter().copied().find(|m| m.http_payload().is_some())
}

/// `true` when `schema` has an event stream or streaming blob (including RPC members without `@httpPayload`).
pub(super) fn has_streaming_payload(schema: &Schema<'_>) -> bool {
    schema.members().iter().any(|member| member.streaming())
}

/// The `Content-Type` rules for a REST request with this input.
///
/// A `@httpPayload` member fixes the expected type: `@mediaType` when present, `text/plain` for
/// strings, the codec's type for structures and documents, and no check for a blob without a media
/// type or for a streaming payload (the legacy server checks neither). An input with no members
/// must have no `Content-Type` at all, unless the input was modeled by the user (the schema then
/// carries an original name) in which case the header is ignored. Otherwise the codec's type is
/// expected when any member is bound to the body.
pub(super) fn expected_request_content_type(
    input: &Schema<'_>,
    codec_content_type: &'static str,
) -> ExpectedContentType {
    if let Some(payload) = payload_member(input) {
        if payload.streaming() {
            return ExpectedContentType::Skip;
        }
        let media_type = payload.media_type().map(|m| m.value());
        return match (payload.shape_type(), media_type) {
            (ShapeType::Blob, None) => ExpectedContentType::Skip,
            (ShapeType::Blob, Some(media)) => ExpectedContentType::Expect(parse_mime(media)),
            (ShapeType::String, media) => ExpectedContentType::Expect(parse_mime(media.unwrap_or("text/plain"))),
            _ => ExpectedContentType::Expect(parse_mime(codec_content_type)),
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
        ExpectedContentType::Expect(parse_mime(codec_content_type))
    } else {
        ExpectedContentType::Skip
    }
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
pub(super) fn accept_permits(headers: &Headers, content_type: &mime::Mime) -> bool {
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
pub(super) fn check_accept(headers: &Headers, expected: &mime::Mime) -> Result<(), DeserializeError> {
    if accept_permits(headers, expected) {
        Ok(())
    } else {
        Err(DeserializeError::NotAcceptable)
    }
}

/// Rejects the request when `expected` is a media type its `Accept` header cannot accept.
pub(super) fn enforce_expected_accept(headers: &Headers, expected: Option<&str>) -> Result<(), DeserializeError> {
    match expected {
        Some(expected) => check_accept(headers, &parse_mime(expected)),
        None => Ok(()),
    }
}

/// Builds the request deserializer for an RPC protocol.
///
/// The body is parsed only when it is non-empty and the input has members: an input without
/// members ignores the body entirely, and an empty body reads as a structure with no members
/// present so that `@required` is enforced by the builder.
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
