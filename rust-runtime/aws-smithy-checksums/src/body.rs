/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::http::{new_from_algorithm, HttpChecksum};

use aws_smithy_http::body::SdkBody;
use aws_smithy_http::header::append_merge_header_maps;

use bytes::Bytes;
use http::{HeaderMap, HeaderValue};
use http_body::SizeHint;
use pin_project_lite::pin_project;

use std::fmt::Display;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    /// A `ChecksumBody` will read and calculate a request body as it's being sent. Once the body has
    /// been completely read, it'll append a trailer with the calculated checksum.
    pub struct ChecksumBody<InnerBody> {
            #[pin]
            body: InnerBody,
            checksum: Option<Box<dyn HttpChecksum>>,
    }
}

impl ChecksumBody<SdkBody> {
    /// Given an `SdkBody` and a `Box<dyn HttpChecksum>`, create a new `ChecksumBody<SdkBody>`.
    pub fn new(body: SdkBody, checksum: Box<dyn HttpChecksum>) -> Self {
        Self {
            body,
            checksum: Some(checksum),
        }
    }
}

impl http_body::Body for ChecksumBody<SdkBody> {
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        match this.checksum {
            Some(checksum) => {
                let poll_res = this.body.poll_data(cx);
                if let Poll::Ready(Some(Ok(data))) = &poll_res {
                    checksum.update(data);
                }

                poll_res
            }
            None => unreachable!("This can only fail if poll_data is called again after poll_trailers, which is invalid"),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        let this = self.project();
        let poll_res = this.body.poll_trailers(cx);

        if let Poll::Ready(Ok(maybe_inner_trailers)) = poll_res {
            let checksum_headers = if let Some(checksum) = this.checksum.take() {
                checksum.headers()
            } else {
                return Poll::Ready(Ok(None));
            };

            return match maybe_inner_trailers {
                Some(inner_trailers) => Poll::Ready(Ok(Some(append_merge_header_maps(
                    inner_trailers,
                    checksum_headers,
                )))),
                None => Poll::Ready(Ok(Some(checksum_headers))),
            };
        }

        poll_res
    }

    fn is_end_stream(&self) -> bool {
        // If inner body is finished and we've already consumed the checksum then we must be
        // at the end of the stream.
        self.body.is_end_stream() && self.checksum.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        self.body.size_hint()
    }
}

pin_project! {
    /// A response body that will calculate a checksum as it is read. If all data is read and the
    /// calculated checksum doesn't match a precalculated checksum, this body will emit an
    /// [asw_smithy_http::body::Error].
    pub struct ChecksumValidatedBody<InnerBody> {
        #[pin]
        inner: InnerBody,
        checksum: Option<Box<dyn HttpChecksum>>,
        precalculated_checksum: Bytes,
    }
}

impl ChecksumValidatedBody<SdkBody> {
    /// Given an `SdkBody`, the name of a checksum algorithm as a `&str`, and a precalculated
    /// checksum represented as `Bytes`, create a new `ChecksumValidatedBody<SdkBody>`.
    /// Valid checksum algorithm names are defined in this crate's [root module](super).
    pub fn new(
        body: SdkBody,
        checksum_algorithm: &str,
        precalculated_checksum: Bytes,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            checksum: Some(new_from_algorithm(checksum_algorithm)?),
            inner: body,
            precalculated_checksum,
        })
    }

    fn poll_inner(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Bytes, aws_smithy_http::body::Error>>> {
        use http_body::Body;

        let this = self.project();
        let checksum = this.checksum;

        match this.inner.poll_data(cx) {
            Poll::Ready(Some(Ok(data))) => {
                tracing::trace!(
                    "reading {} bytes from the body and updating the checksum calculation",
                    data.len()
                );
                let checksum = match checksum.as_mut() {
                    Some(checksum) => checksum,
                    None => {
                        unreachable!("The checksum must exist because it's only taken out once the inner body has been completely polled.");
                    }
                };

                checksum.update(&data);
                Poll::Ready(Some(Ok(data)))
            }
            // Once the inner body has stopped returning data, check the checksum
            // and return an error if it doesn't match.
            Poll::Ready(None) => {
                tracing::trace!("finished reading from body, calculating final checksum");
                let checksum = match checksum.take() {
                    Some(checksum) => checksum,
                    None => {
                        // If the checksum was already taken and this was polled again anyways,
                        // then return nothing
                        return Poll::Ready(None);
                    }
                };

                let actual_checksum = checksum.finalize();
                if *this.precalculated_checksum == actual_checksum {
                    Poll::Ready(None)
                } else {
                    // So many parens it's starting to look like LISP
                    Poll::Ready(Some(Err(Box::new(Error::ChecksumMismatch {
                        expected: this.precalculated_checksum.clone(),
                        actual: actual_checksum,
                    }))))
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Errors related to checksum calculation and validation
#[derive(Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Error {
    /// The actual checksum didn't match the expected checksum. The checksummed data has been
    /// altered since the expected checksum was calculated.
    ChecksumMismatch { expected: Bytes, actual: Bytes },
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            Error::ChecksumMismatch { expected, actual } => write!(
                f,
                "body checksum mismatch. expected body checksum to be {} but it was {}",
                hex::encode(expected),
                hex::encode(actual)
            ),
        }
    }
}

impl std::error::Error for Error {}

impl http_body::Body for ChecksumValidatedBody<SdkBody> {
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        self.poll_inner(cx)
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        self.project().inner.poll_trailers(cx)
    }

    // Once the inner body returns true for is_end_stream, we still need to
    // verify the checksum; Therefore, we always return false here.
    fn is_end_stream(&self) -> bool {
        false
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use super::ChecksumBody;
    use crate::http::new_from_algorithm;
    use crate::{
        body::ChecksumValidatedBody,
        http::{CRC_32_HEADER_NAME, CRC_32_NAME},
    };
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_types::base64;
    use bytes::{Buf, Bytes};
    use bytes_utils::SegmentedBuf;
    use http_body::Body;
    use std::io::Read;

    fn header_value_as_checksum_string(header_value: &http::HeaderValue) -> String {
        let decoded_checksum = base64::decode(header_value.to_str().unwrap()).unwrap();
        let decoded_checksum = decoded_checksum
            .into_iter()
            .map(|byte| format!("{:02X?}", byte))
            .collect::<String>();

        format!("0x{}", decoded_checksum)
    }

    #[tokio::test]
    async fn test_checksum_body() {
        let input_text = "This is some test text for an SdkBody";
        let body = SdkBody::from(input_text);
        let checksum = new_from_algorithm(CRC_32_NAME).unwrap();
        let mut body = ChecksumBody::new(body, checksum);

        let mut output = SegmentedBuf::new();
        while let Some(buf) = body.data().await {
            output.push(buf.unwrap());
        }

        let mut output_text = String::new();
        output
            .reader()
            .read_to_string(&mut output_text)
            .expect("Doesn't cause IO errors");
        // Verify data is complete and unaltered
        assert_eq!(input_text, output_text);

        let trailers = body
            .trailers()
            .await
            .expect("checksum generation was without error")
            .expect("trailers were set");
        let checksum_trailer = trailers
            .get(&CRC_32_HEADER_NAME)
            .expect("trailers contain crc32 checksum");
        let checksum_trailer = header_value_as_checksum_string(checksum_trailer);

        // Known correct checksum for the input "This is some test text for an SdkBody"
        assert_eq!("0x99B01F72", checksum_trailer);
    }

    fn calculate_crc32_checksum(input: &str) -> Bytes {
        let checksum = crc32fast::hash(input.as_bytes());
        Bytes::copy_from_slice(&checksum.to_be_bytes())
    }

    #[tokio::test]
    async fn test_checksum_validated_body_errors_on_mismatch() {
        let input_text = "This is some test text for an SdkBody";
        let actual_checksum = calculate_crc32_checksum(input_text);
        let body = SdkBody::from(input_text);
        let non_matching_checksum = Bytes::copy_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        let mut body =
            ChecksumValidatedBody::new(body, "crc32", non_matching_checksum.clone()).unwrap();

        while let Some(data) = body.data().await {
            match data {
                Ok(_) => { /* Do nothing */ }
                Err(e) => {
                    let expected_error_message = format!(
                        "body checksum mismatch. expected body checksum to be {:x} but it was {:x}",
                        non_matching_checksum, actual_checksum
                    );
                    let actual_error_message = e.to_string();
                    assert_eq!(expected_error_message, actual_error_message);

                    return;
                }
            }
        }

        panic!("didn't hit expected error condition");
    }

    #[tokio::test]
    async fn test_checksum_validated_body_succeeds_on_match() {
        let input_text = "This is some test text for an SdkBody";
        let actual_checksum = calculate_crc32_checksum(input_text);
        let body = SdkBody::from(input_text);
        let mut body = ChecksumValidatedBody::new(body, "crc32", actual_checksum).unwrap();

        let mut output = SegmentedBuf::new();
        while let Some(buf) = body.data().await {
            output.push(buf.unwrap());
        }

        let mut output_text = String::new();
        output
            .reader()
            .read_to_string(&mut output_text)
            .expect("Doesn't cause IO errors");
        // Verify data is complete and unaltered
        assert_eq!(input_text, output_text);
    }
}
