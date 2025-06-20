/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Functionality for calculating the checksum of an HTTP body and emitting it as trailers.

use crate::http::HttpChecksum;

use aws_smithy_http::header::{append_merge_header_maps, append_merge_header_maps_http_1x};
use aws_smithy_types::body::SdkBody;
use pin_project_lite::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

pin_project! {
    /// A body-wrapper that will calculate the `InnerBody`'s checksum and emit it as a trailer.
    pub struct ChecksumBody<InnerBody> {
            #[pin]
            body: InnerBody,
            checksum: Option<Box<dyn HttpChecksum>>,
            written_trailers: bool,
    }
}

impl ChecksumBody<SdkBody> {
    /// Given an `SdkBody` and a `Box<dyn HttpChecksum>`, create a new `ChecksumBody<SdkBody>`.
    pub fn new(body: SdkBody, checksum: Box<dyn HttpChecksum>) -> Self {
        Self {
            body,
            checksum: Some(checksum),
            written_trailers: false,
        }
    }
}

impl http_body::Body for ChecksumBody<SdkBody> {
    type Data = bytes::Bytes;
    type Error = aws_smithy_types::body::Error;

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
    ) -> Poll<Result<Option<http::HeaderMap>, Self::Error>> {
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

    fn size_hint(&self) -> http_body::SizeHint {
        self.body.size_hint()
    }
}

impl http_body_1x::Body for ChecksumBody<SdkBody> {
    type Data = bytes::Bytes;
    type Error = aws_smithy_types::body::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
        println!("LNJ http_1x ChecksumBody");
        let this = self.project();
        let poll_res = this.body.poll_frame(cx);
        println!("POLL_RES: {poll_res:#?}");

        match &poll_res {
            Poll::Ready(Some(Ok(frame))) => {
                // Update checksum for data frames
                if frame.is_data() {
                    if let Some(checksum) = this.checksum {
                        checksum.update(frame.data_ref().expect("Data frame has data"));
                    }
                } else {
                    // Add checksum trailer to other trailers if necessary
                    let checksum_headers = if let Some(checksum) = this.checksum.take() {
                        checksum.headers_http_1x()
                    } else {
                        return Poll::Ready(None);
                    };
                    let trailers = frame
                        .trailers_ref()
                        .expect("Trailers frame has trailers")
                        .clone();
                    *this.written_trailers = true;
                    return Poll::Ready(Some(Ok(http_body_1x::Frame::trailers(
                        append_merge_header_maps_http_1x(trailers, checksum_headers),
                    ))));
                }
            }
            Poll::Ready(None) => {
                // If the trailers have not already been written (because there were no existing
                // trailers on the body) we write them here
                if !*this.written_trailers {
                    let checksum_headers = if let Some(checksum) = this.checksum.take() {
                        checksum.headers_http_1x()
                    } else {
                        return Poll::Ready(None);
                    };
                    let trailers = http_1x::HeaderMap::new();
                    return Poll::Ready(Some(Ok(http_body_1x::Frame::trailers(
                        append_merge_header_maps_http_1x(trailers, checksum_headers),
                    ))));
                }
            }
            _ => {}
        };

        poll_res
    }
}

#[cfg(test)]
mod tests {
    use super::ChecksumBody;
    use crate::{http::CRC_32_HEADER_NAME, ChecksumAlgorithm, CRC_32_NAME};
    use aws_smithy_types::base64;
    use aws_smithy_types::body::SdkBody;
    use bytes::Buf;
    use bytes_utils::SegmentedBuf;
    use http_body::Body;
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
        let input_text = "This is some test text for an SdkBody";
        let body = SdkBody::from(input_text);
        let checksum = CRC_32_NAME
            .parse::<ChecksumAlgorithm>()
            .unwrap()
            .into_impl();
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
            .get(CRC_32_HEADER_NAME)
            .expect("trailers contain crc32 checksum");
        let checksum_trailer = header_value_as_checksum_string(checksum_trailer);

        // Known correct checksum for the input "This is some test text for an SdkBody"
        assert_eq!("0x99B01F72", checksum_trailer);
    }
}
