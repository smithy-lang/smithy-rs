/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! HTTP body-wrappers that perform request compression

// Putting this in a `mod` since I expect we'll have to handle response
// decompression some day.
pub mod compress {
    //! Functionality for compressing an HTTP request body.

    use crate::http::RequestCompressor;
    use aws_smithy_types::body::SdkBody;
    use http::HeaderMap;
    use http_body::SizeHint;
    use pin_project_lite::pin_project;
    use std::pin::Pin;
    use std::task::{Context, Poll};

    pin_project! {
        pub struct CompressedBody<InnerBody> {
            #[pin]
            body: InnerBody,
            request_compressor: Box<dyn RequestCompressor>,
        }
    }

    impl CompressedBody<SdkBody> {
        /// Given an `SdkBody` and a `Box<dyn RequestCompressor>`, create a new `CompressedBody<SdkBody>`.
        pub fn new(body: SdkBody, request_compressor: Box<dyn RequestCompressor>) -> Self {
            Self {
                body,
                request_compressor,
            }
        }
    }

    impl http_body::Body for CompressedBody<SdkBody> {
        type Data = bytes::Bytes;
        type Error = aws_smithy_types::body::Error;

        fn poll_data(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
            let this = self.project();
            match this.body.poll_data(cx)? {
                Poll::Ready(Some(data)) => {
                    let mut out = Vec::new();
                    this.request_compressor
                        .compress_bytes(&data[..], &mut out)?;
                    Poll::Ready(Some(Ok(out.into())))
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }

        fn poll_trailers(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
            let this = self.project();
            this.body.poll_trailers(cx)
        }

        fn is_end_stream(&self) -> bool {
            self.body.is_end_stream()
        }

        fn size_hint(&self) -> SizeHint {
            self.body.size_hint()
        }
    }
}

#[cfg(test)]
mod test {
    use super::compress::CompressedBody;
    use crate::{CompressionAlgorithm, CompressionOptions};
    use aws_smithy_types::body::SdkBody;
    use bytes::Buf;
    use bytes_utils::SegmentedBuf;
    use http_body::Body;
    use pretty_assertions::assert_eq;
    use std::io::Read;

    const UNCOMPRESSED_INPUT: &[u8] = b"hello world";
    const COMPRESSED_OUTPUT: &[u8] = &[
        31, 139, 8, 0, 0, 0, 0, 0, 0, 255, 203, 72, 205, 201, 201, 87, 40, 207, 47, 202, 73, 1, 0,
        133, 17, 74, 13, 11, 0, 0, 0,
    ];

    #[tokio::test]
    async fn test_compressed_body() {
        let compression_algorithm = CompressionAlgorithm::Gzip;
        let compression_options = CompressionOptions::default()
            .with_min_compression_size_bytes(0)
            .unwrap();
        let request_compressor = compression_algorithm.into_impl(&compression_options);
        let body = SdkBody::from(UNCOMPRESSED_INPUT);
        let mut compressed_body = CompressedBody::new(body, request_compressor);

        let mut output = SegmentedBuf::new();
        while let Some(buf) = compressed_body.data().await {
            output.push(buf.unwrap());
        }

        let mut actual_output = Vec::new();
        output
            .reader()
            .read_to_end(&mut actual_output)
            .expect("Doesn't cause IO errors");
        // Verify data is compressed as expected
        assert_eq!(COMPRESSED_OUTPUT, actual_output);
    }
}
