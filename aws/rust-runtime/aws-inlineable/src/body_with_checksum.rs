/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions for modifying requests and responses for the purposes of checksum validation

use std::fmt::Formatter;

/// Given a `&mut http::request::Request`, and checksum algorithm name, calculate a checksum and
/// then modify the request to include the checksum as a header.
pub fn build_checksum_validated_request(
    request: &mut http::request::Request<aws_smithy_http::body::SdkBody>,
    checksum_algorithm: &str,
) -> Result<(), aws_smithy_http::operation::BuildError> {
    let data = request.body().bytes().unwrap_or_default();

    let mut checksum = aws_smithy_checksums::new_checksum(checksum_algorithm);
    checksum
        .update(data)
        .map_err(|err| aws_smithy_http::operation::BuildError::Other(err))?;
    let checksum = checksum
        .finalize()
        .map_err(|err| aws_smithy_http::operation::BuildError::Other(err))?;

    request.headers_mut().insert(
        aws_smithy_checksums::checksum_algorithm_to_checksum_header_name(checksum_algorithm),
        aws_smithy_types::base64::encode(&checksum[..])
            .parse()
            .expect("base64-encoded checksums are always valid header values"),
    );

    Ok(())
}

/// Errors related to constructing checksum-validated HTTP requests
#[derive(Debug)]
pub enum ChecksumValidatedRequestError {
    /// Only request bodies with a known size can be checksum validated
    UnsizedRequestBody,
}

impl std::fmt::Display for ChecksumValidatedRequestError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsizedRequestBody => write!(
                f,
                "Only request bodies with a known size can be checksum validated."
            ),
        }
    }
}

impl std::error::Error for ChecksumValidatedRequestError {}

/// Given an `http::request::Builder`, `SdkBody`, and a checksum algorithm name, return a
/// `Request<SdkBody>` with checksum trailers where the content is `aws-chunked` encoded.
pub fn build_checksum_validated_request_with_streaming_body(
    request: &mut http::request::Request<aws_smithy_http::body::SdkBody>,
    checksum_algorithm: &str,
) -> Result<(), aws_smithy_http::operation::BuildError> {
    use http_body::Body;

    let original_body_size = request.body().size_hint().exact().ok_or_else(|| {
        aws_smithy_http::operation::BuildError::Other(Box::new(
            ChecksumValidatedRequestError::UnsizedRequestBody,
        ))
    })?;
    let body = std::mem::replace(request.body_mut(), aws_smithy_http::body::SdkBody::taken());
    let body = aws_smithy_checksums::body::ChecksumBody::new(body, checksum_algorithm);
    let checksum_trailer_name = body.trailer_name();
    let aws_chunked_body_options = aws_http::content_encoding::AwsChunkedBodyOptions::new()
        .with_stream_length(original_body_size)
        .with_trailer_len(body.trailer_length());

    let body = aws_http::content_encoding::AwsChunkedBody::new(body, aws_chunked_body_options);
    let encoded_content_length = body.size_hint().exact().ok_or_else(|| {
        aws_smithy_http::operation::BuildError::Other(Box::new(
            ChecksumValidatedRequestError::UnsizedRequestBody,
        ))
    })?;
    let headers = request.headers_mut();
    headers.insert(
        http::header::CONTENT_LENGTH,
        http::HeaderValue::from(encoded_content_length),
    );
    headers.insert(
        http::header::HeaderName::from_static("x-amz-decoded-content-length"),
        http::HeaderValue::from(original_body_size),
    );
    headers.insert(
        http::header::HeaderName::from_static("x-amz-trailer"),
        checksum_trailer_name.into(),
    );
    headers.insert(
        http::header::CONTENT_ENCODING,
        http::HeaderValue::from_str(aws_http::content_encoding::header_value::AWS_CHUNKED)
            .map_err(|err| aws_smithy_http::operation::BuildError::Other(Box::new(err)))?,
    );

    let mut body =
        aws_smithy_http::body::SdkBody::from_dyn(http_body::combinators::BoxBody::new(body));

    std::mem::swap(request.body_mut(), &mut body);

    Ok(())
}

/// Given a `Response<SdkBody>`, checksum algorithm name, and pre-calculated checksum, return a
/// `Response<SdkBody>` where the body will processed with the checksum algorithm and checked
/// against the pre-calculated checksum.
pub fn build_checksum_validated_sdk_body(
    body: aws_smithy_http::body::SdkBody,
    checksum_algorithm: &str,
    precalculated_checksum: bytes::Bytes,
) -> aws_smithy_http::body::SdkBody {
    let body = aws_smithy_checksums::body::ChecksumValidatedBody::new(
        body,
        checksum_algorithm,
        precalculated_checksum.clone(),
    );
    aws_smithy_http::body::SdkBody::from_dyn(http_body::combinators::BoxBody::new(body))
}

/// Given the name of a checksum algorithm and a `HeaderMap`, extract the checksum value from the
/// corresponding header as `Some(Bytes)`. If the header is unset, return `None`.
pub fn check_headers_for_precalculated_checksum(
    headers: &http::HeaderMap<http::HeaderValue>,
) -> Option<(&'static str, bytes::Bytes)> {
    for header_name in aws_smithy_checksums::CHECKSUM_HEADERS_IN_PRIORITY_ORDER {
        if let Some(precalculated_checksum) = headers.get(&header_name) {
            let checksum_algorithm =
                aws_smithy_checksums::checksum_header_name_to_checksum_algorithm(&header_name);
            let precalculated_checksum =
                bytes::Bytes::copy_from_slice(precalculated_checksum.as_bytes());

            return Some((checksum_algorithm, precalculated_checksum));
        }
    }

    None
}
