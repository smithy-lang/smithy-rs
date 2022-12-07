/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functionality for calculating the checksum of an HTTP body and emitting it as trailers.

use crate::http::HttpChecksum;
use aws_smithy_http::body::Error;
use aws_smithy_http::body::SdkBody;
use aws_smithy_http::header::append_merge_header_maps;
use bytes::Bytes;
use http_body::{Frame, SizeHint};
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    /// A body-wrapper that will calculate the `InnerBody`'s checksum and emit it as a trailer.
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
    type Error = Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        // If the checksum has already been taken, then we've already returned
        // trailers. There is nothing left to do, so early return.
        if self.checksum.is_none() {
            return Poll::Ready(None);
        }

        let this = self.project();
        match this.body.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    let checksum = this
                        .checksum
                        .as_mut()
                        .expect("function early returns if checksum is taken");
                    checksum.update(data);
                    Poll::Ready(Some(Ok(frame)))
                } else {
                    if let Some(trailers) = frame.into_trailers() {
                        let checksum_headers = this
                            .checksum
                            .take()
                            .expect("function early returns if checksum is taken")
                            .headers();
                        Poll::Ready(Some(Ok(Frame::trailers(append_merge_header_maps(
                            trailers,
                            checksum_headers,
                        )))))
                    } else {
                        panic!("encountered unsupported frame type that wasn't data or trailers");
                    }
                }
            }
            Poll::Ready(None) => {
                let checksum_headers = this
                    .checksum
                    .take()
                    .expect("function early returns if checksum is taken")
                    .headers();
                Poll::Ready(Some(Ok(Frame::trailers(checksum_headers))))
            }
            poll_result => poll_result,
        }
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

#[cfg(test)]
mod tests {
    use super::ChecksumBody;
    use crate::{http::CRC_32_HEADER_NAME, ChecksumAlgorithm, CRC_32_NAME};
    use aws_smithy_http::body::SdkBody;
    use aws_smithy_types::base64;
    use http_body_util::BodyExt;

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
        let checksum = CRC_32_NAME
            .parse::<ChecksumAlgorithm>()
            .unwrap()
            .into_impl();
        let body = ChecksumBody::new(body, checksum);

        let collect = body.collect().await.expect("success");
        let checksum_trailer = collect
            .trailers()
            .expect("trailers")
            .get(&CRC_32_HEADER_NAME)
            .expect("trailers contain crc32 checksum");
        let checksum_trailer = header_value_as_checksum_string(checksum_trailer);

        // Known correct checksum for the input "This is some test text for an SdkBody"
        assert_eq!("0x99B01F72", checksum_trailer);

        let output = collect.to_bytes();
        let output_text = std::str::from_utf8(output.as_ref()).unwrap();

        // Verify data is complete and unaltered
        assert_eq!(input_text, output_text);
    }
}
