/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions for modifying requests and responses for the purposes of checksum validation

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
        aws_smithy_checksums::algorithm_to_header_name(checksum_algorithm),
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
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

    let original_body_size = http_body::Body::size_hint(request.body())
        .exact()
        .ok_or_else(|| {
            aws_smithy_http::operation::BuildError::Other(Box::new(
                ChecksumValidatedRequestError::UnsizedRequestBody,
            ))
        })?;

    let body = std::mem::replace(request.body_mut(), aws_smithy_http::body::SdkBody::taken());
    let checksum_algorithm = checksum_algorithm.to_owned();

    let headers = request.headers_mut();
    headers.insert(
        http::header::HeaderName::from_static("x-amz-trailer"),
        aws_smithy_checksums::algorithm_to_header_name(&checksum_algorithm).into(),
    );

    let mut body = body.map(move |body| {
        let body = aws_smithy_checksums::body::ChecksumBody::new(body, &checksum_algorithm);
        let aws_chunked_body_options = aws_http::content_encoding::AwsChunkedBodyOptions::new()
            .with_stream_length(original_body_size)
            .with_trailer_len(body.trailer_length());

        let body = aws_http::content_encoding::AwsChunkedBody::new(body, aws_chunked_body_options);

        aws_smithy_http::body::SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(body))
    });

    let encoded_content_length = body
        .size_hint()
        .exact()
        .ok_or_else(|| {
            aws_smithy_http::operation::BuildError::Other(Box::new(
                ChecksumValidatedRequestError::UnsizedRequestBody,
            ))
        })
        .expect("unsized bodies are not supported");

    headers.insert(
        http::header::CONTENT_LENGTH,
        http::HeaderValue::from(encoded_content_length),
    );
    headers.insert(
        http::header::HeaderName::from_static("x-amz-decoded-content-length"),
        http::HeaderValue::from(original_body_size),
    );
    headers.insert(
        http::header::CONTENT_ENCODING,
        http::HeaderValue::from_str(aws_http::content_encoding::header_value::AWS_CHUNKED)
            .map_err(|err| aws_smithy_http::operation::BuildError::Other(Box::new(err)))
            .expect("\"aws-chunked\" will always be a valid HeaderValue"),
    );

    std::mem::swap(request.body_mut(), &mut body);

    Ok(())
}

/// Given an `SdkBody`, checksum algorithm name, and pre-calculated checksum, return an
/// `SdkBody` where the body will processed with the checksum algorithm and checked
/// against the pre-calculated checksum.
pub fn build_checksum_validated_sdk_body(
    body: aws_smithy_http::body::SdkBody,
    checksum_algorithm: &str,
    precalculated_checksum: bytes::Bytes,
) -> aws_smithy_http::body::SdkBody {
    tracing::trace!(
        "wrapping response body in {} checksum validator, expected checksum is {:#?}",
        checksum_algorithm,
        precalculated_checksum,
    );

    let checksum_algorithm = checksum_algorithm.to_owned();
    body.map(move |body| {
        aws_smithy_http::body::SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(
            aws_smithy_checksums::body::ChecksumValidatedBody::new(
                body,
                &checksum_algorithm,
                precalculated_checksum.clone(),
            ),
        ))
    })
}

/// Given the name of a checksum algorithm and a `HeaderMap`, extract the checksum value from the
/// corresponding header as `Some(Bytes)`. If the header is unset, return `None`.
pub fn check_headers_for_precalculated_checksum(
    headers: &http::HeaderMap<http::HeaderValue>,
    response_algorithms: &[&str],
) -> Option<(&'static str, bytes::Bytes)> {
    let checksum_headers_to_check = aws_smithy_checksums::CHECKSUM_ALGORITHMS_IN_PRIORITY_ORDER
        .into_iter()
        // Process list of algorithms, from fastest to slowest, that may have been used to checksum
        // the response body, ignoring any that aren't marked as supported algorithms by the model.
        .flat_map(|algo| {
            response_algorithms
                .iter()
                .find(|res_algo| algo.eq_ignore_ascii_case(res_algo))
        })
        // Convert algorithms to `HeaderName`s
        .map(|algo| aws_smithy_checksums::algorithm_to_header_name(algo));

    for header_name in checksum_headers_to_check {
        if let Some(precalculated_checksum) = headers.get(&header_name) {
            let checksum_algorithm = aws_smithy_checksums::header_name_to_algorithm(&header_name);
            let base64_encoded_precalculated_checksum = precalculated_checksum
                .to_str()
                .expect("base64 uses ASCII characters");

            // TODO this error should get bubbled up. It's not likely a service would send back
            //      invalid base64, but we should still be thorough.
            let precalculated_checksum: bytes::Bytes =
                aws_smithy_types::base64::decode(base64_encoded_precalculated_checksum)
                    .unwrap()
                    .into();

            return Some((checksum_algorithm, precalculated_checksum));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use crate::body_with_checksum::build_checksum_validated_sdk_body;
    use aws_smithy_checksums::new_checksum;
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_http::byte_stream::ByteStream;
    use bytes::{Bytes, BytesMut};
    use http_body::Body;
    use std::sync::Once;
    use tempfile::NamedTempFile;

    static INIT_LOGGER: Once = Once::new();
    fn init_logger() {
        INIT_LOGGER.call_once(|| {
            tracing_subscriber::fmt::init();
        });
    }

    #[tokio::test]
    async fn test_checksum_body_is_retryable() {
        let input_text = "Hello world";
        let precalculated_checksum = Bytes::from_static(&[0x8b, 0xd6, 0x9e, 0x52]);
        let body = SdkBody::retryable(move || SdkBody::from(input_text));

        // ensure original SdkBody is retryable
        assert!(body.try_clone().is_some());

        let body = body.map(move |sdk_body| {
            build_checksum_validated_sdk_body(sdk_body, "crc32", precalculated_checksum.clone())
        });

        // ensure wrapped SdkBody is retryable
        let mut body = body.try_clone().expect("body is retryable");

        let mut validated_body = BytesMut::new();

        loop {
            match body.data().await {
                Some(Ok(data)) => validated_body.extend_from_slice(&data),
                Some(Err(err)) => panic!("{}", err),
                None => {
                    break;
                }
            }
        }

        let body = std::str::from_utf8(&validated_body).unwrap();

        // ensure that the wrapped body passes checksum validation
        assert_eq!(input_text, body);
    }

    #[tokio::test]
    async fn test_checksum_body_from_file_is_retryable() {
        use std::io::Write;
        let mut file = NamedTempFile::new().unwrap();
        let mut crc32c_checksum = aws_smithy_checksums::new_checksum("crc32c");

        for i in 0..10000 {
            let line = format!("This is a large file created for testing purposes {}", i);
            file.as_file_mut().write(line.as_bytes()).unwrap();
            crc32c_checksum.update(&line.as_bytes()).unwrap();
        }

        let body = ByteStream::read_from()
            .path(&file)
            .buffer_size(1024)
            .build()
            .await
            .unwrap();

        let precalculated_checksum = crc32c_checksum.finalize().unwrap();
        let expected_checksum = precalculated_checksum.clone();

        let body = body.map(move |sdk_body| {
            build_checksum_validated_sdk_body(sdk_body, "crc32c", precalculated_checksum.clone())
        });

        // ensure wrapped SdkBody is retryable
        let mut body = body.into_inner().try_clone().expect("body is retryable");

        let mut validated_body = BytesMut::new();

        // If this loop completes, then it means the body's checksum was valid, but let's calculate
        // a checksum again just in case.
        let mut redundant_crc32c_checksum = aws_smithy_checksums::new_checksum("crc32c");
        loop {
            match body.data().await {
                Some(Ok(data)) => {
                    redundant_crc32c_checksum.update(&data).unwrap();
                    validated_body.extend_from_slice(&data);
                }
                Some(Err(err)) => panic!("{}", err),
                None => {
                    break;
                }
            }
        }

        let actual_checksum = redundant_crc32c_checksum.finalize().unwrap();
        assert_eq!(expected_checksum, actual_checksum);

        // Ensure the file's checksum isn't the same as an empty checksum. This way, we'll know that
        // data was actually processed.
        let unexpected_checksum = new_checksum("crc32c").finalize().unwrap();
        assert_ne!(unexpected_checksum, actual_checksum);
    }

    #[tokio::test]
    async fn test_build_checksum_validated_body_works() {
        init_logger();

        let input_text = "Hello world";
        let precalculated_checksum = Bytes::from_static(&[0x8b, 0xd6, 0x9e, 0x52]);
        let body = ByteStream::new(SdkBody::from(input_text));

        let body = body.map(move |sdk_body| {
            build_checksum_validated_sdk_body(sdk_body, "crc32", precalculated_checksum.clone())
        });

        let mut validated_body = Vec::new();
        if let Err(e) = tokio::io::copy(&mut body.into_async_read(), &mut validated_body).await {
            tracing::error!("{}", e);
            panic!("checksum validation has failed");
        };
        let body = std::str::from_utf8(&validated_body).unwrap();

        assert_eq!(input_text, body);
    }
}
