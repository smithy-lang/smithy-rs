/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use bytes::Bytes;
use http::{HeaderMap, HeaderValue};
use http_body::{Body, SizeHint};
use pin_project_lite::pin_project;

use std::pin::Pin;
use std::task::{Context, Poll};

const CRLF: &str = "\r\n";
const CHUNK_TERMINATOR: &str = "0\r\n";

/// Content encoding header value constants
pub mod header_value {
    /// Header value denoting "aws-chunked" encoding
    pub const AWS_CHUNKED: &str = "aws-chunked";
}

/// Options used when constructing an [`AwsChunkedBody`][AwsChunkedBody].
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct AwsChunkedBodyOptions {
    /// The total size of the stream. Because we only support unsigned encoding
    /// this implies that there will only be a single chunk containing the
    /// underlying payload.
    pub stream_length: u64,
    /// The length of each trailer sent within an `AwsChunkedBody`. Necessary in
    /// order to correctly calculate the total size of the body accurately.
    pub trailer_lens: Vec<u64>,
}

impl AwsChunkedBodyOptions {
    /// Create a new [`AwsChunkedBodyOptions`][AwsChunkedBodyOptions]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set stream length
    pub fn with_stream_length(mut self, stream_length: u64) -> Self {
        self.stream_length = stream_length;
        self
    }

    /// Set a trailer len
    pub fn with_trailer_len(mut self, trailer_len: u64) -> Self {
        self.trailer_lens.push(trailer_len);
        self
    }
}

#[derive(Debug, PartialEq, Eq)]
enum AwsChunkedBodyState {
    /// Write out the size of the chunk that will follow. Then, transition into the
    /// `WritingChunk` state.
    WritingChunkSize,
    /// Write out the next chunk of data. Multiple polls of the inner body may need to occur before
    /// all data is written out. Once there is no more data to write, transition into the
    /// `WritingTrailers` state.
    WritingChunk,
    /// Write out all trailers associated with this `AwsChunkedBody` and then transition into the
    /// `Closed` state.
    WritingTrailers,
    /// This is the final state. Write out the body terminator and then remain in this state.
    Closed,
}

pin_project! {
    /// A request body compatible with `Content-Encoding: aws-chunked`
    ///
    /// Chunked-Body grammar is defined in [ABNF] as:
    ///
    /// ```txt
    /// Chunked-Body    = *chunk
    ///                   last-chunk
    ///                   chunked-trailer
    ///                   CRLF
    ///
    /// chunk           = chunk-size CRLF chunk-data CRLF
    /// chunk-size      = 1*HEXDIG
    /// last-chunk      = 1*("0") CRLF
    /// chunked-trailer = *( entity-header CRLF )
    /// entity-header   = field-name ":" OWS field-value OWS
    /// ```
    /// For more info on what the abbreviations mean, see https://datatracker.ietf.org/doc/html/rfc7230#section-1.2
    ///
    /// [ABNF]:https://en.wikipedia.org/wiki/Augmented_Backus%E2%80%93Naur_form
    #[derive(Debug)]
    pub struct AwsChunkedBody<InnerBody> {
        #[pin]
        inner: InnerBody,
        #[pin]
        state: AwsChunkedBodyState,
        options: AwsChunkedBodyOptions,
    }
}

impl<Inner> AwsChunkedBody<Inner> {
    /// Wrap the given body in an outer body compatible with `Content-Encoding: aws-chunked`
    pub fn new(body: Inner, options: AwsChunkedBodyOptions) -> Self {
        Self {
            inner: body,
            state: AwsChunkedBodyState::WritingChunkSize,
            options,
        }
    }

    fn encoded_length(&self) -> Option<u64> {
        let mut length = 0;
        if self.options.stream_length != 0 {
            length += get_unsigned_chunk_bytes_length(self.options.stream_length);
        }

        // End chunk
        length += CHUNK_TERMINATOR.len() as u64;

        // Trailers
        for len in self.options.trailer_lens.iter() {
            length += len + CRLF.len() as u64;
        }

        // Encoding terminator
        length += CRLF.len() as u64;

        Some(length)
    }
}

fn get_unsigned_chunk_bytes_length(payload_length: u64) -> u64 {
    let hex_repr_len = int_log16(payload_length);
    hex_repr_len + CRLF.len() as u64 + payload_length + CRLF.len() as u64
}

fn trailers_as_aws_chunked_bytes(
    total_length_of_trailers_in_bytes: u64,
    trailer_map: Option<HeaderMap>,
) -> bytes::Bytes {
    use std::fmt::Write;

    // On 32-bit operating systems, we might not be able to convert the u64 to a usize, so we just
    // use `String::new` in that case.
    let mut trailers = match usize::try_from(total_length_of_trailers_in_bytes) {
        Ok(total_length_of_trailers_in_bytes) => {
            String::with_capacity(total_length_of_trailers_in_bytes)
        }
        Err(_) => String::new(),
    };
    let mut already_wrote_first_trailer = false;

    if let Some(trailer_map) = trailer_map {
        for (header_name, header_value) in trailer_map.into_iter() {
            match header_name {
                // New name, new value
                Some(header_name) => {
                    if already_wrote_first_trailer {
                        // First trailer shouldn't have a preceding CRLF, but every trailer after it should
                        trailers.write_str(CRLF).unwrap();
                    } else {
                        already_wrote_first_trailer = true;
                    }

                    trailers.write_str(header_name.as_str()).unwrap();
                    trailers.write_char(':').unwrap();
                }
                // Same name, new value
                None => {
                    trailers.write_char(',').unwrap();
                }
            }
            trailers.write_str(header_value.to_str().unwrap()).unwrap();
        }
    }

    // Write CRLF to end the body
    trailers.write_str(CRLF).unwrap();
    // If we wrote at least one trailer, we need to write an extra CRLF
    if total_length_of_trailers_in_bytes != 0 {
        trailers.write_str(CRLF).unwrap();
    }

    trailers.into()
}

impl<Inner: Body<Data = Bytes, Error = aws_smithy_http::body::Error>> Body
    for AwsChunkedBody<Inner>
{
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        tracing::trace!("polling AwsChunkedBody");
        let mut this = self.project();

        match *this.state {
            AwsChunkedBodyState::WritingChunkSize => {
                tracing::trace!("writing chunk size");
                *this.state = AwsChunkedBodyState::WritingChunk;
                // A chunk must be prefixed by chunk size in hexadecimal
                let chunk_size = Bytes::from(format!("{:X?}\r\n", this.options.stream_length));
                Poll::Ready(Some(Ok(chunk_size)))
            }
            AwsChunkedBodyState::WritingChunk => match this.inner.poll_data(cx) {
                Poll::Ready(Some(Ok(data))) => {
                    tracing::trace!("writing chunk data");
                    Poll::Ready(Some(Ok(data)))
                }
                Poll::Ready(None) => {
                    tracing::trace!("no more chunk data, writing CRLF and terminator chunk");
                    *this.state = AwsChunkedBodyState::WritingTrailers;
                    Poll::Ready(Some(Ok(Bytes::from([CRLF, CHUNK_TERMINATOR].concat()))))
                }
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
                Poll::Pending => Poll::Pending,
            },
            AwsChunkedBodyState::WritingTrailers => {
                return match this.inner.poll_trailers(cx) {
                    Poll::Ready(Ok(trailers)) => {
                        *this.state = AwsChunkedBodyState::Closed;
                        let total_length_of_trailers_in_bytes =
                            this.options.trailer_lens.iter().sum();

                        Poll::Ready(Some(Ok(trailers_as_aws_chunked_bytes(
                            total_length_of_trailers_in_bytes,
                            trailers,
                        ))))
                    }
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
                };
            }
            AwsChunkedBodyState::Closed => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
        // Trailers were already appended to the body because of the content encoding scheme
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        self.state == AwsChunkedBodyState::Closed
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(
            self.encoded_length()
                .expect("Requests made with aws-chunked encoding must have known size")
                as u64,
        )
    }
}

// Used for finding how many hexadecimal digits it takes to represent a base 10 integer
fn int_log16<T>(mut i: T) -> u64
where
    T: std::ops::DivAssign + PartialOrd + From<u8> + Copy,
{
    let mut len = 0;
    let zero = T::from(0);
    let sixteen = T::from(16);

    while i > zero {
        i /= sixteen;
        len += 1;
    }

    len
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_http::body::SdkBody;
    use bytes::Buf;
    use bytes_utils::SegmentedBuf;
    use std::io::Read;
    use std::time::Duration;

    #[tokio::test]
    async fn test_aws_chunked_encoding() {
        let test_fut = async {
            let input_str = "Hello world";
            let opts = AwsChunkedBodyOptions::new().with_stream_length(input_str.len() as u64);
            let mut body = AwsChunkedBody::new(SdkBody::from(input_str), opts);

            let mut output = SegmentedBuf::new();
            while let Some(buf) = body.data().await {
                output.push(buf.unwrap());
            }

            let mut actual_output = String::new();
            output
                .reader()
                .read_to_string(&mut actual_output)
                .expect("Doesn't cause IO errors");

            let expected_output = "B\r\nHello world\r\n0\r\n\r\n";

            assert_eq!(expected_output, actual_output);
            assert!(
                body.trailers()
                    .await
                    .expect("no errors occurred during trailer polling")
                    .is_none(),
                "aws-chunked encoded bodies don't have normal HTTP trailers"
            );

            // You can insert a `tokio::time::sleep` here to verify the timeout works as intended
        };

        let timeout_duration = Duration::from_secs(3);
        if let Err(_) = tokio::time::timeout(timeout_duration, test_fut).await {
            panic!("test_aws_chunked_encoding timed out after {timeout_duration:?}");
        }
    }
}
