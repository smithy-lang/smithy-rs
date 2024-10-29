/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functionality for calculating the checksum of an HTTP body and emitting it as trailers.

use crate::http::HttpChecksum;

use aws_smithy_http::header::append_merge_header_maps;
use aws_smithy_types::body::SdkBody;

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
    type Data = bytes::Bytes;
    type Error = aws_smithy_types::body::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.project();

        match this.body.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) if frame.is_data() => {
                let data = frame.into_data().expect("unreachable");
                match this.checksum {
                    Some(checksum) => {
                        checksum.update(&data);
                        Poll::Ready(Some(Ok(Frame::data(data))))
                    }
                    None => unreachable!("This can only fail if poll_data is called again after poll_trailers, which is invalid"),
                }
            }
            Poll::Ready(Some(Ok(non_data_frame))) => match non_data_frame.into_trailers() {
                Ok(inner_trailers) => {
                    if let Some(checksum) = this.checksum.take() {
                        let merged = append_merge_header_maps(inner_trailers, checksum.headers());
                        Poll::Ready(Some(Ok(Frame::trailers(merged))))
                    } else {
                        Poll::Ready(Some(Ok(Frame::trailers(inner_trailers))))
                    }
                }
                Err(non_trailer_frame) => Poll::Ready(Some(Ok(non_trailer_frame))),
            },
            Poll::Ready(None) => {
                if let Some(checksum) = this.checksum.take() {
                    Poll::Ready(Some(Ok(Frame::trailers(checksum.headers()))))
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
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
    use aws_smithy_types::base64;
    use aws_smithy_types::body::SdkBody;
    use bytes::Buf;
    use std::fmt::Write;
    use std::io::Read;

    fn header_value_as_checksum_string(header_value: &http::HeaderValue) -> String {
        let decoded_checksum = base64::decode(header_value.to_str().unwrap()).unwrap();
        let decoded_checksum = decoded_checksum
            .into_iter()
            .fold(String::new(), |mut acc, byte| {
                write!(acc, "{byte:02X?}").expect("string will always be writeable");
                acc
            });

        format!("0x{}", decoded_checksum)
    }

    #[tokio::test]
    async fn test_checksum_body() {
        use http_body_util::BodyExt;
        let input_text = "This is some test text for an SdkBody";
        let body = SdkBody::from(input_text);
        let checksum = CRC_32_NAME
            .parse::<ChecksumAlgorithm>()
            .unwrap()
            .into_impl();
        let body = ChecksumBody::new(body, checksum);

        let collected = body.collect().await.expect("body and trailers valid");

        let trailers = collected.trailers().expect("trailers were set").to_owned();
        let checksum_trailer = trailers
            .get(CRC_32_HEADER_NAME)
            .expect("trailers contain crc32 checksum");
        let checksum_trailer = header_value_as_checksum_string(checksum_trailer);

        // Known correct checksum for the input "This is some test text for an SdkBody"
        assert_eq!("0x99B01F72", checksum_trailer);

        let mut output_text = String::new();
        collected
            .to_bytes()
            .reader()
            .read_to_string(&mut output_text)
            .expect("Doesn't cause IO errors");
        // Verify data is complete and unaltered
        assert_eq!(input_text, output_text);
    }
}
