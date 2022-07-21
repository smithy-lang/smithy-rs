/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functions for modifying requests and responses for the purposes of checksum validation

/// Given a `&mut http::request::Request`, and a boxed `HttpChecksum`, calculate a checksum and
/// modify the request to include the checksum as a header.
///
/// **NOTE: This function only supports non-streaming request bodies and will return an error if a
/// streaming request body is passed.**
pub(crate) fn calculate_body_checksum_and_insert_as_header(
    request: &mut http::request::Request<aws_smithy_http::body::SdkBody>,
    mut checksum: Box<dyn aws_smithy_checksums::http::HttpChecksum>,
) -> Result<(), aws_smithy_http::operation::BuildError> {
    match request.body().bytes() {
        Some(data) => {
            checksum.update(data);

            request
                .headers_mut()
                .insert(checksum.header_name(), checksum.header_value());

            Ok(())
        }
        None => {
            let err = Box::new(Error::ChecksumHeadersAreUnsupportedForStreamingBody);
            Err(aws_smithy_http::operation::BuildError::Other(err))
        }
    }
}

/// Errors related to constructing checksum-validated HTTP requests
#[derive(Debug)]
pub(crate) enum Error {
    /// Only request bodies with a known size can be checksum validated
    UnsizedRequestBody,
    ChecksumHeadersAreUnsupportedForStreamingBody,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsizedRequestBody => write!(
                f,
                "Only request bodies with a known size can be checksum validated."
            ),
            Self::ChecksumHeadersAreUnsupportedForStreamingBody => write!(
                f,
                "Checksum header insertion is only supported for non-streaming HTTP bodies. \
                 To checksum validate a streaming body, the checksums must be sent as trailers."
            ),
        }
    }
}

impl std::error::Error for Error {}

pub(crate) fn build_streaming_request_body_with_checksum_trailers(
    request: &mut http::request::Request<aws_smithy_http::body::SdkBody>,
    checksum_algorithm: &str,
) -> Result<(), aws_smithy_http::operation::BuildError> {
    use aws_http::content_encoding::{AwsChunkedBody, AwsChunkedBodyOptions};
    use aws_smithy_checksums::{body::calculate, http::HttpChecksum};
    use http_body::Body;

    // Ensure that a valid `checksum_algorithm` was passed, emitting an error if that's not the case
    if let Err(err) = aws_smithy_checksums::http::new_from_algorithm(&checksum_algorithm) {
        return Err(aws_smithy_http::operation::BuildError::Other(err));
    }

    let original_body_size = request.body().size_hint().exact().ok_or_else(|| {
        aws_smithy_http::operation::BuildError::Other(Box::new(Error::UnsizedRequestBody))
    })?;

    let mut body = {
        let body = std::mem::replace(request.body_mut(), aws_smithy_http::body::SdkBody::taken());

        let checksum_algorithm = checksum_algorithm.to_owned();
        body.map(move |body| {
            // This function takes the checksum algorithm's name instead of a pre-built checksum because
            // the map fn could be called multiple times if the `SdkBody` we're wrapping here is
            // retryable.
            let checksum = aws_smithy_checksums::http::new_from_algorithm(&checksum_algorithm)
                .expect(
                "this can't fail because we already asserted that the algorithm name was valid.",
            );

            let trailer_len = HttpChecksum::size(checksum.as_ref());
            let body = calculate::ChecksumBody::new(body, checksum);
            let aws_chunked_body_options =
                AwsChunkedBodyOptions::new(original_body_size, vec![trailer_len]);

            let body = AwsChunkedBody::new(body, aws_chunked_body_options);

            aws_smithy_http::body::SdkBody::from_dyn(aws_smithy_http::body::BoxBody::new(body))
        })
    };

    let encoded_content_length = body.size_hint().exact().ok_or_else(|| {
        aws_smithy_http::operation::BuildError::Other(Box::new(Error::UnsizedRequestBody))
    })?;

    let headers = request.headers_mut();

    headers.insert(
        http::header::HeaderName::from_static("x-amz-trailer"),
        aws_smithy_checksums::http::algorithm_to_header_name(checksum_algorithm)
            .expect("build_streaming_request_body_with_checksum_trailers is only ever called with modeled algorithms").into(),
    );

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
pub(crate) fn build_checksum_validated_sdk_body(
    body: aws_smithy_http::body::SdkBody,
    checksum_algorithm: &str,
    precalculated_checksum: bytes::Bytes,
) -> Result<aws_smithy_http::body::SdkBody, aws_smithy_http::operation::BuildError> {
    use aws_smithy_checksums::body::validate;
    use aws_smithy_http::body::{BoxBody, SdkBody};

    let checksum_algorithm = checksum_algorithm.to_owned();
    // Ensure that a valid `checksum_algorithm` was passed, emitting an error if that's not the case
    if let Err(err) = aws_smithy_checksums::http::new_from_algorithm(&checksum_algorithm) {
        return Err(aws_smithy_http::operation::BuildError::Other(err));
    }

    Ok(body.map(move |body| {
        // This function takes the checksum algorithm's name instead of a pre-built checksum because
        // the map fn could be called multiple times if the `SdkBody` we're wrapping here is
        // retryable.
        let checksum = aws_smithy_checksums::http::new_from_algorithm(&checksum_algorithm).expect(
            "this can't fail because we already asserted that the algorithm name was valid.",
        );
        SdkBody::from_dyn(BoxBody::new(validate::ChecksumBody::new(
            body,
            checksum,
            precalculated_checksum.clone(),
        )))
    }))
}

/// Given the name of a checksum algorithm and a `HeaderMap`, extract the checksum value from the
/// corresponding header as `Some(Bytes)`. If the header is unset, return `None`.
pub(crate) fn check_headers_for_precalculated_checksum(
    headers: &http::HeaderMap<http::HeaderValue>,
    response_algorithms: &[&str],
) -> Option<(&'static str, bytes::Bytes)> {
    let checksum_algorithms_to_check =
        aws_smithy_checksums::http::CHECKSUM_ALGORITHMS_IN_PRIORITY_ORDER
            .into_iter()
            // Process list of algorithms, from fastest to slowest, that may have been used to checksum
            // the response body, ignoring any that aren't marked as supported algorithms by the model.
            .flat_map(|algo| {
                // For loop is necessary b/c the compiler doesn't infer the correct lifetimes for iter().find()
                for res_algo in response_algorithms {
                    if algo.eq_ignore_ascii_case(res_algo) {
                        return Some(algo);
                    }
                }

                None
            });

    for checksum_algorithm in checksum_algorithms_to_check {
        let header_name = aws_smithy_checksums::http::algorithm_to_header_name(checksum_algorithm)
            .expect(
            "CHECKSUM_ALGORITHMS_IN_PRIORITY_ORDER only contains valid checksum algorithm names",
        );
        if let Some(precalculated_checksum) = headers.get(&header_name) {
            let base64_encoded_precalculated_checksum = precalculated_checksum
                .to_str()
                .expect("base64 uses ASCII characters");

            let precalculated_checksum: bytes::Bytes =
                aws_smithy_types::base64::decode(base64_encoded_precalculated_checksum)
                    .expect("services will always base64 encode the checksum value per the spec")
                    .into();

            return Some((checksum_algorithm, precalculated_checksum));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::build_checksum_validated_sdk_body;
    use aws_smithy_checksums::http::new_from_algorithm;
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
                .unwrap()
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
        let mut crc32c_checksum = new_from_algorithm("crc32c").unwrap();

        for i in 0..10000 {
            let line = format!("This is a large file created for testing purposes {}", i);
            file.as_file_mut().write(line.as_bytes()).unwrap();
            crc32c_checksum.update(&line.as_bytes());
        }

        let body = ByteStream::read_from()
            .path(&file)
            .buffer_size(1024)
            .build()
            .await
            .unwrap();

        let precalculated_checksum = crc32c_checksum.finalize();
        let expected_checksum = precalculated_checksum.clone();

        let body = body.map(move |sdk_body| {
            build_checksum_validated_sdk_body(sdk_body, "crc32c", precalculated_checksum.clone())
                .unwrap()
        });

        // ensure wrapped SdkBody is retryable
        let mut body = body.into_inner().try_clone().expect("body is retryable");

        let mut validated_body = BytesMut::new();

        // If this loop completes, then it means the body's checksum was valid, but let's calculate
        // a checksum again just in case.
        let mut redundant_crc32c_checksum = new_from_algorithm("crc32c").unwrap();
        loop {
            match body.data().await {
                Some(Ok(data)) => {
                    redundant_crc32c_checksum.update(&data);
                    validated_body.extend_from_slice(&data);
                }
                Some(Err(err)) => panic!("{}", err),
                None => {
                    break;
                }
            }
        }

        let actual_checksum = redundant_crc32c_checksum.finalize();
        assert_eq!(expected_checksum, actual_checksum);

        // Ensure the file's checksum isn't the same as an empty checksum. This way, we'll know that
        // data was actually processed.
        let unexpected_checksum = new_from_algorithm("crc32c").unwrap().finalize();
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
                .unwrap()
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
