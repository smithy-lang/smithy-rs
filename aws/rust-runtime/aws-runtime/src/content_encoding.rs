/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_sigv4::http_request::SigningError;
use aws_smithy_runtime_api::http::Headers;
use aws_smithy_types::config_bag::{Storable, StoreReplace};
use bytes::{Buf, Bytes, BytesMut};
use bytes_utils::SegmentedBuf;
use pin_project_lite::pin_project;

use std::pin::Pin;
use std::sync::{mpsc, Arc, Mutex};
use std::task::{Context, Poll};

const CRLF: &str = "\r\n";
const CRLF_RAW: &[u8] = b"\r\n";

const CHUNK_SIGNATURE_BEGIN: &str = ";chunk-signature=";
const CHUNK_SIGNATURE_BEGIN_RAW: &[u8] = b";chunk-signature=";

const CHUNK_TERMINATOR: &str = "0\r\n";
const CHUNK_TERMINATOR_RAW: &[u8] = b"0\r\n";

const TRAILER_SEPARATOR: &[u8] = b":";

const FIXED_CHUNK_SIZE_BYTE: usize = 64 * 1024; // 64 KB

const SIGNATURE_LENGTH: usize = 64;

/// Content encoding header name constants
pub mod header {
    /// Header name denoting "x-amz-trailer-signature"
    pub const X_AMZ_TRAILER_SIGNATURE: &str = "x-amz-trailer-signature";
}

/// Content encoding header value constants
pub mod header_value {
    /// Header value denoting "aws-chunked" encoding
    pub const AWS_CHUNKED: &str = "aws-chunked";
}

// Trait for signing chunks and trailers
//
// Trait methods take `&mut self`` because they keep track of running signature as they sign each chunk.
pub(crate) trait SignChunk: std::fmt::Debug {
    fn chunk_signature(&mut self, chunk: &Bytes) -> Result<String, SigningError>;

    fn trailer_signature(&mut self, trailing_headers: &Headers) -> Result<String, SigningError>;
}

/// Deferred chunk signer that allows a signer to be wired up later.
///
/// Signing chunks and trailers occurs after HTTP request signing and requires
/// signing context from the initial HTTP signature operation.
///
/// This signer establishes an MPSC channel with a sender placed in the request
/// configuration. The HTTP signer implementation retrieves the sender from the
/// config and sends an actual signing implementation with the required context.
#[derive(Clone, Debug)]
#[allow(clippy::type_complexity)]
pub struct DeferredSigner {
    // The outer `Arc` enables cloning `DeferredSigner`, making `AwsChunkedBody` retryable.
    // The inner trait objects are boxed to enable calling mutable trait methods.
    signer: Arc<Mutex<Option<Box<dyn SignChunk + Send + Sync>>>>,
    rx: Arc<Mutex<Option<mpsc::Receiver<Box<dyn SignChunk + Send + Sync>>>>>,
}

impl Storable for DeferredSigner {
    type Storer = StoreReplace<Self>;
}

impl DeferredSigner {
    /// Create a new `DeferredSigner` and its associated sender.
    pub fn new() -> (Self, DeferredSignerSender) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                signer: Default::default(),
                rx: Arc::new(Mutex::new(Some(rx))),
            },
            DeferredSignerSender { tx: Mutex::new(tx) },
        )
    }

    /// Create an empty `DeferredSigner`, typically used as a placeholder for `std::mem::replace`
    pub fn empty() -> Self {
        Self {
            rx: Default::default(),
            signer: Default::default(),
        }
    }

    fn acquire(&self) -> Box<dyn SignChunk + Send + Sync> {
        let mut rx = self.rx.lock().unwrap();
        rx.take()
            .and_then(|receiver| receiver.try_recv().ok())
            .expect("signer should be available")
    }
}

/// A sender placed in the config bag to wire up a signer for signing chunks and trailers.
#[derive(Debug)]
pub struct DeferredSignerSender {
    tx: Mutex<mpsc::Sender<Box<dyn SignChunk + Send + Sync>>>,
}

impl DeferredSignerSender {
    pub(crate) fn send(
        &self,
        signer: Box<dyn SignChunk + Send + Sync>,
    ) -> Result<(), mpsc::SendError<Box<dyn SignChunk + Send + Sync>>> {
        self.tx.lock().unwrap().send(signer)
    }
}

impl Storable for DeferredSignerSender {
    type Storer = StoreReplace<Self>;
}

impl SignChunk for DeferredSigner {
    fn chunk_signature(&mut self, chunk: &Bytes) -> Result<String, SigningError> {
        let mut signer = self.signer.lock().unwrap();
        let signer = signer.get_or_insert_with(|| self.acquire());
        signer.chunk_signature(chunk)
    }

    fn trailer_signature(&mut self, trailing_headers: &Headers) -> Result<String, SigningError> {
        let mut signer = self.signer.lock().unwrap();
        let signer = signer.get_or_insert_with(|| self.acquire());
        signer.trailer_signature(trailing_headers)
    }
}

/// Options used when constructing an [`AwsChunkedBody`].
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct AwsChunkedBodyOptions {
    /// The total size of the stream. Because we only support unsigned encoding
    /// this implies that there will only be a single chunk containing the
    /// underlying payload.
    stream_length: u64,
    /// The length of each trailer sent within an `AwsChunkedBody`. Necessary in
    /// order to correctly calculate the total size of the body accurately.
    trailer_lengths: Vec<u64>,
    /// Whether the aws-chunked encoding is disabled. This could occur, for instance,
    /// if a user specifies a custom checksum, rendering aws-chunked encoding unnecessary.
    disabled: bool,
    /// Whether chunks and trailer are signed.
    is_signed: bool,
    /// The size of each chunk in bytes, only for testing.
    chunk_size: Option<usize>,
}

impl Storable for AwsChunkedBodyOptions {
    type Storer = StoreReplace<Self>;
}

impl AwsChunkedBodyOptions {
    /// Create a new [`AwsChunkedBodyOptions`].
    pub fn new(stream_length: u64, trailer_lengths: Vec<u64>) -> Self {
        Self {
            stream_length,
            trailer_lengths,
            disabled: false,
            is_signed: false,
            chunk_size: None,
        }
    }

    #[allow(dead_code)] // for testing
    fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.chunk_size = Some(chunk_size);
        self
    }

    fn chunk_size(&self) -> usize {
        self.chunk_size.unwrap_or(FIXED_CHUNK_SIZE_BYTE)
    }

    fn total_trailer_length(&self) -> u64 {
        self.trailer_lengths.iter().sum::<u64>()
            // We need to account for a CRLF after each trailer name/value pair
            + (self.trailer_lengths.len() * CRLF.len()) as u64
    }

    /// Set the stream length in the options
    pub fn with_stream_length(mut self, stream_length: u64) -> Self {
        self.stream_length = stream_length;
        self
    }

    /// Append a trailer length to the options
    pub fn with_trailer_len(mut self, trailer_len: u64) -> Self {
        self.trailer_lengths.push(trailer_len);
        self
    }

    /// Return whether there are no trailers
    pub fn is_trailer_empty(&self) -> bool {
        self.trailer_lengths.is_empty()
    }

    /// Create a new [`AwsChunkedBodyOptions`] with aws-chunked encoding disabled.
    ///
    /// When the option is disabled, the body must not be wrapped in an `AwsChunkedBody`.
    pub fn disable_chunked_encoding() -> Self {
        Self {
            disabled: true,
            ..Default::default()
        }
    }

    /// Return whether aws-chunked encoding is disabled.
    pub fn disabled(&self) -> bool {
        self.disabled
    }

    /// Set whether to use signed chunked encoding
    pub fn signed_chunked_encoding(mut self, is_signed: bool) -> Self {
        self.is_signed = is_signed;
        self
    }

    /// Return the length of the body after `aws-chunked` encoding is applied
    pub fn encoded_length(&self) -> u64 {
        if self.is_signed {
            self.signed_encoded_length()
        } else {
            self.unsigned_encoded_length()
        }
    }

    fn signed_encoded_length(&self) -> u64 {
        let number_of_data_chunks = self.stream_length / self.chunk_size() as u64;
        let remaining_data_chunk = self.stream_length % self.chunk_size() as u64;

        let mut length = number_of_data_chunks
            * get_signed_chunk_bytes_length(self.chunk_size() as u64)
            + if remaining_data_chunk > 0 {
                get_signed_chunk_bytes_length(remaining_data_chunk)
            } else {
                0
            };

        // End chunk
        length += get_signed_chunk_bytes_length(0);

        length -= CRLF.len() as u64; // The last CRLF is not needed for 0-sized signed chunk

        // Trailers
        for len in self.trailer_lengths.iter() {
            length += len + CRLF.len() as u64;
        }

        // Encoding terminator
        length += CRLF.len() as u64;

        length
    }

    fn unsigned_encoded_length(&self) -> u64 {
        let number_of_data_chunks = self.stream_length / self.chunk_size() as u64;
        let remaining_data_chunk = self.stream_length % self.chunk_size() as u64;

        let mut length = number_of_data_chunks
            * get_unsigned_chunk_bytes_length(self.chunk_size() as u64)
            + if remaining_data_chunk > 0 {
                get_unsigned_chunk_bytes_length(remaining_data_chunk)
            } else {
                0
            };

        // End chunk
        length += CHUNK_TERMINATOR.len() as u64;

        // Trailers
        for len in self.trailer_lengths.iter() {
            length += len + CRLF.len() as u64;
        }

        // Encoding terminator
        length += CRLF.len() as u64;

        length
    }
}

#[derive(Debug)]
enum ChunkBuf {
    /// Nothing has been buffered yet.
    Empty,
    /// Some data has been buffered.
    /// The SegmentedBuf will automatically purge when it reads off the end of a chunk boundary.
    Partial(SegmentedBuf<Bytes>),
    /// The end of the stream has been reached, but there may still be some buffered data.
    EosPartial(SegmentedBuf<Bytes>),
    /// An exception terminated this stream.
    Terminated,
}

impl ChunkBuf {
    /// Return true if there's more buffered data.
    fn remaining(&self) -> usize {
        match self {
            ChunkBuf::Empty | ChunkBuf::Terminated => 0,
            ChunkBuf::Partial(segments) | ChunkBuf::EosPartial(segments) => segments.remaining(),
        }
    }

    /// Return true if the stream has ended.
    fn is_eos(&self) -> bool {
        matches!(self, ChunkBuf::EosPartial(_) | ChunkBuf::Terminated)
    }

    /// Return a mutable reference to the underlying buffered data.
    fn buffered(&mut self) -> &mut SegmentedBuf<Bytes> {
        match self {
            ChunkBuf::Empty => panic!("buffer must be populated before reading; this is a bug"),
            ChunkBuf::Partial(segmented) => segmented,
            ChunkBuf::EosPartial(segmented) => segmented,
            ChunkBuf::Terminated => panic!("buffer has been terminated; this is a bug"),
        }
    }

    /// Return a `ChunkBuf` that has reached end of stream.
    fn ended(self) -> Self {
        match self {
            ChunkBuf::Empty => ChunkBuf::EosPartial(SegmentedBuf::new()),
            ChunkBuf::Partial(segmented) => ChunkBuf::EosPartial(segmented),
            ChunkBuf::EosPartial(_) => panic!("already end of stream; this is a bug"),
            ChunkBuf::Terminated => panic!("stream terminated; this is a bug"),
        }
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
    /// Write out a zero-sized signed chunk.
    WritingZeroSizedSignedChunk,
    /// Buffer all trailers from the inner body, which avoids assuming trailing headers fit in a single frame.
    PollingTrailers,
    /// Write out all trailers associated with this `AwsChunkedBody` and then transition into the
    /// `Closed` state.
    WritingTrailers,
    /// This is the final state. Write out the body terminator and then remain in this state.
    Closed,
}

pin_project! {
    /// A request body compatible with `Content-Encoding: aws-chunked`.
    ///
    /// See [SigV4 streaming](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-streaming.html)
    /// and [streaming trailers](https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-streaming-trailers.html).
    #[derive(Debug)]
    pub struct AwsChunkedBody<InnerBody> {
        #[pin]
        inner: InnerBody,
        #[pin]
        state: AwsChunkedBodyState,
        options: AwsChunkedBodyOptions,
        inner_body_bytes_read_so_far: usize,
        #[pin]
        chunk_buffer: ChunkBuf,
        #[pin]
        buffered_trailing_headers: Option<http_1x::HeaderMap>,
        #[pin]
        signer: Option<Box<dyn SignChunk + Send + Sync>>,
    }
}

impl<Inner> AwsChunkedBody<Inner> {
    /// Wrap the given body in an outer body compatible with `Content-Encoding: aws-chunked`
    pub fn new(body: Inner, options: AwsChunkedBodyOptions) -> Self {
        Self {
            inner: body,
            state: AwsChunkedBodyState::WritingChunkSize,
            options,
            inner_body_bytes_read_so_far: 0,
            chunk_buffer: ChunkBuf::Empty,
            buffered_trailing_headers: None,
            signer: None,
        }
    }

    /// Set signer for signing chunks and trailers.
    #[allow(private_bounds)] // Until we support chunk signing for a custom signer, the trait does not need to be public
    pub fn with_signer<S>(mut self, signer: S) -> Self
    where
        S: SignChunk + Send + Sync + 'static,
    {
        self.signer = Some(Box::new(signer));
        self
    }

    // Buffer the next chunk from the inner body into the provided `chunk_buffer`, and return
    // whether or not it should continue reading from `inner`.
    //
    // If it has exhausted data frames and started polling trailers, the buffered trailer will be
    // pushed into `buffered_trailing_headers`, immediately marking the `chunk_buffer` as `eos`.
    fn buffer_next_chunk(
        inner: Pin<&mut Inner>,
        mut chunk_buffer: Pin<&mut ChunkBuf>,
        mut buffered_trailing_headers: Pin<&mut Option<http_1x::HeaderMap>>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<bool, aws_smithy_types::body::Error>>
    where
        Inner: http_body_1x::Body<Data = Bytes, Error = aws_smithy_types::body::Error>,
    {
        match inner.poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if frame.is_data() {
                    let data = frame.into_data().expect("just checked to be data");
                    match chunk_buffer.as_mut().get_mut() {
                        ChunkBuf::Empty => {
                            let mut buf = SegmentedBuf::new();
                            buf.push(data);
                            *chunk_buffer.as_mut().get_mut() = ChunkBuf::Partial(buf);
                        }
                        ChunkBuf::Partial(buf) => buf.push(data),
                        ChunkBuf::EosPartial(_) | ChunkBuf::Terminated => {
                            panic!("cannot buffer more data after the stream has ended or been terminated; this is a bug")
                        }
                    }
                    Poll::Ready(Ok(true))
                } else {
                    let buf = chunk_buffer.as_mut().get_mut();
                    *buf = std::mem::replace(buf, ChunkBuf::Empty).ended();
                    *buffered_trailing_headers.as_mut().get_mut() = frame.into_trailers().ok();
                    Poll::Ready(Ok(false))
                }
            }
            Poll::Ready(Some(Err(e))) => {
                *chunk_buffer.as_mut().get_mut() = ChunkBuf::Terminated;
                Poll::Ready(Err(e))
            }
            Poll::Ready(None) => Poll::Ready(Ok(false)),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<Inner> http_body_04x::Body for AwsChunkedBody<Inner>
where
    Inner: http_body_04x::Body<Data = Bytes, Error = aws_smithy_types::body::Error>,
{
    type Data = Bytes;
    type Error = aws_smithy_types::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        tracing::trace!(state = ?self.state, "polling AwsChunkedBody");
        let mut this = self.project();

        use AwsChunkedBodyState::*;
        match *this.state {
            WritingChunkSize => {
                if this.options.stream_length == 0 {
                    // If the stream is empty, we skip to writing trailers after writing the CHUNK_TERMINATOR.
                    *this.state = WritingTrailers;
                    tracing::trace!("stream is empty, writing chunk terminator");
                    Poll::Ready(Some(Ok(Bytes::from([CHUNK_TERMINATOR].concat()))))
                } else {
                    *this.state = WritingChunk;
                    // A chunk must be prefixed by chunk size in hexadecimal
                    let chunk_size = format!("{:X?}{CRLF}", this.options.stream_length);
                    tracing::trace!(%chunk_size, "writing chunk size");
                    let chunk_size = Bytes::from(chunk_size);
                    Poll::Ready(Some(Ok(chunk_size)))
                }
            }
            WritingChunk => match this.inner.poll_data(cx) {
                Poll::Ready(Some(Ok(data))) => {
                    tracing::trace!(len = data.len(), "writing chunk data");
                    *this.inner_body_bytes_read_so_far += data.len();
                    Poll::Ready(Some(Ok(data)))
                }
                Poll::Ready(None) => {
                    let actual_stream_length = *this.inner_body_bytes_read_so_far as u64;
                    let expected_stream_length = this.options.stream_length;
                    if actual_stream_length != expected_stream_length {
                        let err = Box::new(AwsChunkedBodyError::StreamLengthMismatch {
                            actual: actual_stream_length,
                            expected: expected_stream_length,
                        });
                        return Poll::Ready(Some(Err(err)));
                    };

                    tracing::trace!("no more chunk data, writing CRLF and chunk terminator");
                    *this.state = WritingTrailers;
                    // Since we wrote chunk data, we end it with a CRLF and since we only write
                    // a single chunk, we write the CHUNK_TERMINATOR immediately after
                    Poll::Ready(Some(Ok(Bytes::from([CRLF, CHUNK_TERMINATOR].concat()))))
                }
                Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
                Poll::Pending => Poll::Pending,
            },
            WritingZeroSizedSignedChunk | PollingTrailers | WritingTrailers => {
                return match this.inner.poll_trailers(cx) {
                    Poll::Ready(Ok(trailers)) => {
                        *this.state = Closed;
                        let expected_length =
                            http_02x_utils::total_rendered_length_of_trailers(trailers.as_ref());
                        let actual_length = this.options.total_trailer_length();

                        if expected_length != actual_length {
                            let err = AwsChunkedBodyError::ReportedTrailerLengthMismatch {
                                actual: actual_length,
                                expected: expected_length,
                            };
                            return Poll::Ready(Some(Err(err.into())));
                        }

                        let mut trailers = http_02x_utils::trailers_as_aws_chunked_bytes(
                            trailers,
                            actual_length + 1,
                        );
                        // Insert the final CRLF to close the body
                        trailers.extend_from_slice(CRLF.as_bytes());

                        Poll::Ready(Some(Ok(trailers.into())))
                    }
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
                };
            }
            Closed => Poll::Ready(None),
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<Option<http_02x::HeaderMap<http_02x::HeaderValue>>, Self::Error>> {
        // Trailers were already appended to the body because of the content encoding scheme
        Poll::Ready(Ok(None))
    }

    fn is_end_stream(&self) -> bool {
        self.state == AwsChunkedBodyState::Closed
    }

    fn size_hint(&self) -> http_body_04x::SizeHint {
        http_body_04x::SizeHint::with_exact(self.options.encoded_length())
    }
}

/// Utility functions to help with the [http_body_04x::Body] trait implementation
mod http_02x_utils {
    use super::{CRLF, TRAILER_SEPARATOR};
    use bytes::BytesMut;
    use http_02x::HeaderMap;

    /// Writes trailers out into a `string` and then converts that `String` to a `Bytes` before
    /// returning.
    ///
    /// - Trailer names are separated by a single colon only, no space.
    /// - Trailer names with multiple values will be written out one line per value, with the name
    ///   appearing on each line.
    pub(super) fn trailers_as_aws_chunked_bytes(
        trailer_map: Option<HeaderMap>,
        estimated_length: u64,
    ) -> BytesMut {
        if let Some(trailer_map) = trailer_map {
            let mut current_header_name = None;
            let mut trailers =
                BytesMut::with_capacity(estimated_length.try_into().unwrap_or_default());

            for (header_name, header_value) in trailer_map.into_iter() {
                // When a header has multiple values, the name only comes up in iteration the first time
                // we see it. Therefore, we need to keep track of the last name we saw and fall back to
                // it when `header_name == None`.
                current_header_name = header_name.or(current_header_name);

                // In practice, this will always exist, but `if let` is nicer than unwrap
                if let Some(header_name) = current_header_name.as_ref() {
                    trailers.extend_from_slice(header_name.as_ref());
                    trailers.extend_from_slice(TRAILER_SEPARATOR);
                    trailers.extend_from_slice(header_value.as_bytes());
                    trailers.extend_from_slice(CRLF.as_bytes());
                }
            }

            trailers
        } else {
            BytesMut::new()
        }
    }

    /// Given an optional `HeaderMap`, calculate the total number of bytes required to represent the
    /// `HeaderMap`. If no `HeaderMap` is given as input, return 0.
    ///
    /// - Trailer names are separated by a single colon only, no space.
    /// - Trailer names with multiple values will be written out one line per value, with the name
    ///   appearing on each line.
    pub(super) fn total_rendered_length_of_trailers(trailer_map: Option<&HeaderMap>) -> u64 {
        match trailer_map {
            Some(trailer_map) => trailer_map
                .iter()
                .map(|(trailer_name, trailer_value)| {
                    trailer_name.as_str().len()
                        + TRAILER_SEPARATOR.len()
                        + trailer_value.len()
                        + CRLF.len()
                })
                .sum::<usize>() as u64,
            None => 0,
        }
    }
}

/// Implementing the [http_body_1x::Body] trait
impl<Inner> http_body_1x::Body for AwsChunkedBody<Inner>
where
    Inner: http_body_1x::Body<Data = Bytes, Error = aws_smithy_types::body::Error>,
{
    type Data = Bytes;
    type Error = aws_smithy_types::body::Error;

    fn is_end_stream(&self) -> bool {
        self.state == AwsChunkedBodyState::Closed
    }

    fn size_hint(&self) -> http_body_1x::SizeHint {
        http_body_1x::SizeHint::with_exact(self.options.encoded_length())
    }

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
        tracing::trace!(state = ?self.state, "polling AwsChunkedBody");
        let mut this = self.project();
        let chunk_size = this.options.chunk_size();

        use AwsChunkedBodyState::*;
        match *this.state {
            WritingChunkSize | WritingChunk => {
                while !this.chunk_buffer.is_eos() {
                    if this.chunk_buffer.remaining() >= chunk_size {
                        let buf = this.chunk_buffer.buffered();
                        let chunk_bytes = buf.copy_to_bytes(chunk_size);
                        let chunk = if this.options.is_signed {
                            let signer = this.signer.as_deref_mut().expect("signer must be set");
                            http_1x_utils::signed_encoded_chunk(signer, chunk_bytes).map_err(
                                |e| Box::new(AwsChunkedBodyError::FailedToSign { source: e }),
                            )?
                        } else {
                            http_1x_utils::unsigned_encoded_chunk(chunk_bytes)
                        };
                        *this.inner_body_bytes_read_so_far += chunk_size;
                        tracing::trace!("writing chunk data: {:#?}", chunk);
                        return Poll::Ready(Some(Ok(http_body_1x::Frame::data(chunk))));
                    }

                    match Self::buffer_next_chunk(
                        this.inner.as_mut(),
                        this.chunk_buffer.as_mut(),
                        this.buffered_trailing_headers.as_mut(),
                        cx,
                    ) {
                        Poll::Ready(Ok(true)) => continue,
                        Poll::Ready(Ok(false)) => break,
                        Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                        Poll::Pending => return Poll::Pending,
                    }
                }

                if this.chunk_buffer.remaining() > 0 {
                    let bytes_len_to_read =
                        std::cmp::min(this.chunk_buffer.remaining(), chunk_size);
                    let buf = this.chunk_buffer.buffered();
                    let chunk_bytes = buf.copy_to_bytes(bytes_len_to_read);
                    let chunk = if this.options.is_signed {
                        let signer = this.signer.as_deref_mut().expect("signer must be set");
                        http_1x_utils::signed_encoded_chunk(signer, chunk_bytes).map_err(|e| {
                            Box::new(AwsChunkedBodyError::FailedToSign { source: e })
                        })?
                    } else {
                        http_1x_utils::unsigned_encoded_chunk(chunk_bytes)
                    };
                    *this.inner_body_bytes_read_so_far += bytes_len_to_read;
                    tracing::trace!("remaining chunk data: {:#?}", chunk);
                    return Poll::Ready(Some(Ok(http_body_1x::Frame::data(chunk))));
                }

                debug_assert!(this.chunk_buffer.remaining() == 0);

                // We exhausted the body data, now check if the length is correct
                if let Err(poll_stream_len_err) = http_1x_utils::check_for_stream_length_mismatch(
                    *this.inner_body_bytes_read_so_far as u64,
                    this.options.stream_length,
                ) {
                    return poll_stream_len_err;
                }

                if this.options.is_signed {
                    *this.state = WritingZeroSizedSignedChunk;
                } else {
                    *this.state = PollingTrailers;
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            WritingZeroSizedSignedChunk => {
                let signer = this.signer.as_deref_mut().expect("signer must be set");
                let zero_sized_chunk = http_1x_utils::signed_encoded_chunk(signer, Bytes::new())
                    .map_err(|e| Box::new(AwsChunkedBodyError::FailedToSign { source: e }))?;
                if this.buffered_trailing_headers.is_some() {
                    *this.state = PollingTrailers;
                    let mut zero_sized_chunk = BytesMut::from(&zero_sized_chunk[..]);
                    debug_assert!(zero_sized_chunk.ends_with(b"\r\n\r\n"));
                    // For trailing checksum, we do not want the second CRLF as the checksum is appended in-between two CRLFs
                    zero_sized_chunk.truncate(zero_sized_chunk.len() - 2);
                    let zero_sized_chunk = zero_sized_chunk.freeze();
                    tracing::trace!("writing zero sized signed chunk: {:#?}", zero_sized_chunk);
                    Poll::Ready(Some(Ok(http_body_1x::Frame::data(zero_sized_chunk))))
                } else {
                    *this.state = Closed;
                    tracing::trace!(
                        "writing zero sized signed chunk without trailer: {:#?}",
                        zero_sized_chunk
                    );
                    Poll::Ready(Some(Ok(http_body_1x::Frame::data(zero_sized_chunk))))
                }
            }
            PollingTrailers => match this.inner.as_mut().poll_frame(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    let trailers = frame.into_trailers().ok();
                    if let Some(trailers) = trailers {
                        match this.buffered_trailing_headers.as_mut().get_mut() {
                            Some(existing) => existing.extend(trailers),
                            None => {
                                *this.buffered_trailing_headers.as_mut().get_mut() = Some(trailers)
                            }
                        }
                    }
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Poll::Ready(Some(Err(err))) => {
                    tracing::error!(error = ?err, "error polling inner");
                    Poll::Ready(Some(Err(err)))
                }
                Poll::Ready(None) => {
                    *this.state = WritingTrailers;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                Poll::Pending => Poll::Pending,
            },
            WritingTrailers => {
                let mut final_chunk = if this.options.is_signed {
                    BytesMut::new()
                } else {
                    BytesMut::from(CHUNK_TERMINATOR_RAW)
                };

                let trailer_bytes = if let Some(mut trailer) = this.buffered_trailing_headers.take()
                {
                    let mut trailer_bytes = BytesMut::new();
                    let trailer = if this.options.is_signed && !trailer.is_empty() {
                        let signer = this.signer.as_deref_mut().expect("signer must be set");
                        let signature = signer
                            .trailer_signature(&Headers::try_from(trailer.clone())?)
                            .map_err(|e| {
                                Box::new(AwsChunkedBodyError::FailedToSign { source: e })
                            })?;
                        trailer.insert(
                            http_1x::header::HeaderName::from_static(
                                header::X_AMZ_TRAILER_SIGNATURE,
                            ),
                            http_1x::header::HeaderValue::from_str(&signature).unwrap(),
                        );
                        trailer
                    } else {
                        trailer
                    };

                    let actual_length: u64 =
                        http_1x_utils::total_rendered_length_of_trailers(Some(&trailer));
                    let expected_length = this.options.total_trailer_length();
                    if expected_length != actual_length {
                        let err = AwsChunkedBodyError::ReportedTrailerLengthMismatch {
                            actual: actual_length,
                            expected: expected_length,
                        };
                        return Poll::Ready(Some(Err(err.into())));
                    }

                    trailer_bytes =
                        http_1x_utils::trailers_as_aws_chunked_bytes(Some(&trailer), trailer_bytes);
                    trailer_bytes.freeze()
                } else {
                    Bytes::new()
                };

                *this.state = Closed;

                if final_chunk.is_empty() && trailer_bytes.is_empty() {
                    // Case for signed aws-chunked encoding with no trailers
                    return Poll::Ready(None);
                }

                final_chunk.extend_from_slice(&trailer_bytes);
                final_chunk.extend_from_slice(CRLF_RAW);

                tracing::trace!("final chunk: {:#?}", final_chunk);
                Poll::Ready(Some(Ok(http_body_1x::Frame::data(final_chunk.freeze()))))
            }
            Closed => Poll::Ready(None),
        }
    }
}
/// Utility functions to help with the [http_body_1x::Body] trait implementation
mod http_1x_utils {
    use std::task::Poll;

    use crate::content_encoding::{SignChunk, CHUNK_SIGNATURE_BEGIN_RAW};

    use super::{CRLF_RAW, TRAILER_SEPARATOR};
    use aws_sigv4::http_request::SigningError;
    use bytes::{Bytes, BytesMut};
    use http_1x::{HeaderMap, HeaderName};

    pub(super) fn signed_encoded_chunk(
        signer: &mut (dyn SignChunk + Send + Sync),
        chunk_bytes: Bytes,
    ) -> Result<Bytes, SigningError> {
        let chunk_size = format!("{:X}", chunk_bytes.len());
        let mut chunk = bytes::BytesMut::new();
        chunk.extend_from_slice(chunk_size.as_bytes());
        chunk.extend_from_slice(CHUNK_SIGNATURE_BEGIN_RAW);
        chunk.extend_from_slice(signer.chunk_signature(&chunk_bytes)?.as_bytes());
        chunk.extend_from_slice(CRLF_RAW);
        chunk.extend_from_slice(&chunk_bytes);
        chunk.extend_from_slice(CRLF_RAW);
        Ok(chunk.freeze())
    }

    pub(super) fn unsigned_encoded_chunk(chunk_bytes: Bytes) -> Bytes {
        let chunk_size = format!("{:X}", chunk_bytes.len());
        let mut chunk = bytes::BytesMut::new();
        chunk.extend_from_slice(chunk_size.as_bytes());
        chunk.extend_from_slice(CRLF_RAW);
        chunk.extend_from_slice(&chunk_bytes);
        chunk.extend_from_slice(CRLF_RAW);
        chunk.freeze()
    }

    /// Writes trailers out into a byte array `buffer`.
    ///
    /// - Trailer names are separated by a single colon only, no space.
    /// - Trailer names with multiple values will be written out one line per value, with the name
    ///   appearing on each line.
    pub(super) fn trailers_as_aws_chunked_bytes(
        trailer_map: Option<&HeaderMap>,
        mut buffer: BytesMut,
    ) -> BytesMut {
        if let Some(trailer_map) = trailer_map {
            let mut current_header_name: Option<HeaderName> = None;

            for (header_name, header_value) in trailer_map.clone().into_iter() {
                // When a header has multiple values, the name only comes up in iteration the first time
                // we see it. Therefore, we need to keep track of the last name we saw and fall back to
                // it when `header_name == None`.
                current_header_name = header_name.or(current_header_name);

                // In practice, this will always exist, but `if let` is nicer than unwrap
                if let Some(header_name) = current_header_name.as_ref() {
                    buffer.extend_from_slice(header_name.as_ref());
                    buffer.extend_from_slice(TRAILER_SEPARATOR);
                    buffer.extend_from_slice(header_value.as_bytes());
                    buffer.extend_from_slice(CRLF_RAW);
                }
            }

            buffer
        } else {
            buffer
        }
    }

    /// Given an optional `HeaderMap`, calculate the total number of bytes required to represent the
    /// `HeaderMap`. If no `HeaderMap` is given as input, return 0.
    ///
    /// - Trailer names are separated by a single colon only, no space.
    /// - Trailer names with multiple values will be written out one line per value, with the name
    ///   appearing on each line.
    pub(super) fn total_rendered_length_of_trailers(trailer_map: Option<&HeaderMap>) -> u64 {
        match trailer_map {
            Some(trailer_map) => trailer_map
                .iter()
                .map(|(trailer_name, trailer_value)| {
                    trailer_name.as_str().len()
                        + TRAILER_SEPARATOR.len()
                        + trailer_value.len()
                        + CRLF_RAW.len()
                })
                .sum::<usize>() as u64,
            None => 0,
        }
    }

    /// This is an ugly return type, but in practice it just returns `Ok(())` if the values match
    /// and `Err(Poll::Ready(Some(Err(AwsChunkedBodyError::StreamLengthMismatch))))` if they don't
    #[allow(clippy::type_complexity)]
    pub(super) fn check_for_stream_length_mismatch(
        actual_stream_length: u64,
        expected_stream_length: u64,
    ) -> Result<(), Poll<Option<Result<http_body_1x::Frame<Bytes>, aws_smithy_types::body::Error>>>>
    {
        if actual_stream_length != expected_stream_length {
            let err = Box::new(super::AwsChunkedBodyError::StreamLengthMismatch {
                actual: actual_stream_length,
                expected: expected_stream_length,
            });
            return Err(Poll::Ready(Some(Err(err))));
        };

        Ok(())
    }
}

/// Errors related to `AwsChunkedBody`
#[derive(Debug)]
enum AwsChunkedBodyError {
    /// Error that occurs when the sum of `trailer_lengths` set when creating an `AwsChunkedBody` is
    /// not equal to the actual length of the trailers returned by the inner `http_body::Body`
    /// implementor. These trailer lengths are necessary in order to correctly calculate the total
    /// size of the body for setting the content length header.
    ReportedTrailerLengthMismatch { actual: u64, expected: u64 },
    /// Error that occurs when the `stream_length` set when creating an `AwsChunkedBody` is not
    /// equal to the actual length of the body returned by the inner `http_body::Body` implementor.
    /// `stream_length` must be correct in order to set an accurate content length header.
    StreamLengthMismatch { actual: u64, expected: u64 },
    /// Error that occurs when signing a chunk fails.
    FailedToSign { source: SigningError },
}

impl std::fmt::Display for AwsChunkedBodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ReportedTrailerLengthMismatch { actual, expected } => {
                write!(f, "When creating this AwsChunkedBody, length of trailers was reported as {expected}. However, when double checking during trailer encoding, length was found to be {actual} instead.")
            }
            Self::StreamLengthMismatch { actual, expected } => {
                write!(f, "When creating this AwsChunkedBody, stream length was reported as {expected}. However, when double checking during body encoding, length was found to be {actual} instead.")
            }
            Self::FailedToSign { source } => {
                write!(f, "Signing error during aws-chunked encoding: {source}")
            }
        }
    }
}

impl std::error::Error for AwsChunkedBodyError {}

// Used for finding how many hexadecimal digits it takes to represent a base 10 integer
fn int_log16<T>(mut i: T) -> u64
where
    T: std::ops::DivAssign + PartialOrd + From<u8> + Copy,
{
    let mut len = 0;
    let zero = T::from(0);
    let sixteen = T::from(16);

    // Handle an edge case where 0 is passed in, which still requires 1 hex digit to represent
    if i == zero {
        return 1;
    }

    while i > zero {
        i /= sixteen;
        len += 1;
    }

    len
}

// Return the length of a signed chunk:
//
// A signed chunk looks like:
// 10000;chunk-signature=b474d8862b1487a5145d686f57f013e54db672cee1c953b3010fb58501ef5aa2\r\n
// <65536-bytes>\r\n
fn get_signed_chunk_bytes_length(payload_length: u64) -> u64 {
    let hex_repr_len = int_log16(payload_length);
    hex_repr_len
        + CHUNK_SIGNATURE_BEGIN.len() as u64
        + SIGNATURE_LENGTH as u64
        + CRLF.len() as u64
        + payload_length
        + CRLF.len() as u64
}

// Return the length of an unsigned chunk:
//
// An unsigned chunk looks like:
// 10000\r\n
// <65536-bytes>\r\n
fn get_unsigned_chunk_bytes_length(payload_length: u64) -> u64 {
    let hex_repr_len = int_log16(payload_length);
    hex_repr_len + CRLF.len() as u64 + payload_length + CRLF.len() as u64
}

#[cfg(test)]
mod tests {
    use super::int_log16;

    #[test]
    fn test_int_log16() {
        assert_eq!(int_log16(0u64), 1); // 0x0
        assert_eq!(int_log16(1u64), 1); // 0x1
        assert_eq!(int_log16(15u64), 1); // 0xF
        assert_eq!(int_log16(16u64), 2); // 0x10
        assert_eq!(int_log16(255u64), 2); // 0xFF
        assert_eq!(int_log16(256u64), 3); // 0x100
        assert_eq!(int_log16(4095u64), 3); // 0xFFF
        assert_eq!(int_log16(4096u64), 4); // 0x1000
        assert_eq!(int_log16(65535u64), 4); // 0xFFFF
        assert_eq!(int_log16(65536u64), 5); // 0x10000
        assert_eq!(int_log16(1048575u64), 5); // 0xFFFFF
        assert_eq!(int_log16(1048576u64), 6); // 0x100000
        assert_eq!(int_log16(u64::MAX), 16); // 0xFFFFFFFFFFFFFFFF
    }

    #[cfg(test)]
    mod http_02x_tests {
        use super::super::{
            http_02x_utils::{total_rendered_length_of_trailers, trailers_as_aws_chunked_bytes},
            AwsChunkedBody, AwsChunkedBodyOptions, CHUNK_TERMINATOR, CRLF,
        };

        use aws_smithy_types::body::SdkBody;
        use bytes::{Buf, Bytes};
        use bytes_utils::SegmentedBuf;
        use http_02x::{HeaderMap, HeaderValue};
        use http_body_04x::{Body, SizeHint};
        use pin_project_lite::pin_project;

        use std::io::Read;
        use std::pin::Pin;
        use std::task::{Context, Poll};
        use std::time::Duration;

        pin_project! {
            struct SputteringBody {
                parts: Vec<Option<Bytes>>,
                cursor: usize,
                delay_in_millis: u64,
            }
        }

        impl SputteringBody {
            fn len(&self) -> usize {
                self.parts.iter().flatten().map(|b| b.len()).sum()
            }
        }

        impl Body for SputteringBody {
            type Data = Bytes;
            type Error = aws_smithy_types::body::Error;

            fn poll_data(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
                if self.cursor == self.parts.len() {
                    return Poll::Ready(None);
                }

                let this = self.project();
                let delay_in_millis = *this.delay_in_millis;
                let next_part = this.parts.get_mut(*this.cursor).unwrap().take();

                match next_part {
                    None => {
                        *this.cursor += 1;
                        let waker = cx.waker().clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(delay_in_millis)).await;
                            waker.wake();
                        });
                        Poll::Pending
                    }
                    Some(data) => {
                        *this.cursor += 1;
                        Poll::Ready(Some(Ok(data)))
                    }
                }
            }

            fn poll_trailers(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Result<Option<HeaderMap<HeaderValue>>, Self::Error>> {
                Poll::Ready(Ok(None))
            }

            fn is_end_stream(&self) -> bool {
                false
            }

            fn size_hint(&self) -> SizeHint {
                SizeHint::new()
            }
        }

        #[tokio::test]
        async fn test_aws_chunked_encoding() {
            let test_fut = async {
                let input_str = "Hello world";
                let opts = AwsChunkedBodyOptions::new(input_str.len() as u64, Vec::new());
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
            if tokio::time::timeout(timeout_duration, test_fut)
                .await
                .is_err()
            {
                panic!("test_aws_chunked_encoding timed out after {timeout_duration:?}");
            }
        }

        #[tokio::test]
        async fn test_aws_chunked_encoding_sputtering_body() {
            let test_fut = async {
                let input = SputteringBody {
                    parts: vec![
                        Some(Bytes::from_static(b"chunk 1, ")),
                        None,
                        Some(Bytes::from_static(b"chunk 2, ")),
                        Some(Bytes::from_static(b"chunk 3, ")),
                        None,
                        None,
                        Some(Bytes::from_static(b"chunk 4, ")),
                        Some(Bytes::from_static(b"chunk 5, ")),
                        Some(Bytes::from_static(b"chunk 6")),
                    ],
                    cursor: 0,
                    delay_in_millis: 500,
                };
                let opts = AwsChunkedBodyOptions::new(input.len() as u64, Vec::new());
                let mut body = AwsChunkedBody::new(input, opts);

                let mut output = SegmentedBuf::new();
                while let Some(buf) = body.data().await {
                    output.push(buf.unwrap());
                }

                let mut actual_output = String::new();
                output
                    .reader()
                    .read_to_string(&mut actual_output)
                    .expect("Doesn't cause IO errors");

                let expected_output =
                    "34\r\nchunk 1, chunk 2, chunk 3, chunk 4, chunk 5, chunk 6\r\n0\r\n\r\n";

                assert_eq!(expected_output, actual_output);
                assert!(
                    body.trailers()
                        .await
                        .expect("no errors occurred during trailer polling")
                        .is_none(),
                    "aws-chunked encoded bodies don't have normal HTTP trailers"
                );
            };

            let timeout_duration = Duration::from_secs(3);
            if tokio::time::timeout(timeout_duration, test_fut)
                .await
                .is_err()
            {
                panic!(
                "test_aws_chunked_encoding_sputtering_body timed out after {timeout_duration:?}"
            );
            }
        }

        #[tokio::test]
        #[should_panic = "called `Result::unwrap()` on an `Err` value: ReportedTrailerLengthMismatch { actual: 44, expected: 0 }"]
        async fn test_aws_chunked_encoding_incorrect_trailer_length_panic() {
            let input_str = "Hello world";
            // Test body has no trailers, so this length is incorrect and will trigger an assert panic
            // When the panic occurs, it will actually expect a length of 44. This is because, when using
            // aws-chunked encoding, each trailer will end with a CRLF which is 2 bytes long.
            let wrong_trailer_len = 42;
            let opts = AwsChunkedBodyOptions::new(input_str.len() as u64, vec![wrong_trailer_len]);
            let mut body = AwsChunkedBody::new(SdkBody::from(input_str), opts);

            // We don't care about the body contents but we have to read it all before checking for trailers
            while let Some(buf) = body.data().await {
                drop(buf.unwrap());
            }

            assert!(
                body.trailers()
                    .await
                    .expect("no errors occurred during trailer polling")
                    .is_none(),
                "aws-chunked encoded bodies don't have normal HTTP trailers"
            );
        }

        #[tokio::test]
        async fn test_aws_chunked_encoding_empty_body() {
            let input_str = "";
            let opts = AwsChunkedBodyOptions::new(input_str.len() as u64, Vec::new());
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

            let expected_output = [CHUNK_TERMINATOR, CRLF].concat();

            assert_eq!(expected_output, actual_output);
            assert!(
                body.trailers()
                    .await
                    .expect("no errors occurred during trailer polling")
                    .is_none(),
                "aws-chunked encoded bodies don't have normal HTTP trailers"
            );
        }

        #[tokio::test]
        async fn test_total_rendered_length_of_trailers() {
            let mut trailers = HeaderMap::new();

            trailers.insert("empty_value", HeaderValue::from_static(""));

            trailers.insert("single_value", HeaderValue::from_static("value 1"));

            trailers.insert("two_values", HeaderValue::from_static("value 1"));
            trailers.append("two_values", HeaderValue::from_static("value 2"));

            trailers.insert("three_values", HeaderValue::from_static("value 1"));
            trailers.append("three_values", HeaderValue::from_static("value 2"));
            trailers.append("three_values", HeaderValue::from_static("value 3"));

            let trailers = Some(trailers);
            let actual_length = total_rendered_length_of_trailers(trailers.as_ref());
            let expected_length =
                (trailers_as_aws_chunked_bytes(trailers, actual_length).len()) as u64;

            assert_eq!(expected_length, actual_length);
        }

        #[tokio::test]
        async fn test_total_rendered_length_of_empty_trailers() {
            let trailers = Some(HeaderMap::new());
            let actual_length = total_rendered_length_of_trailers(trailers.as_ref());
            let expected_length =
                (trailers_as_aws_chunked_bytes(trailers, actual_length).len()) as u64;

            assert_eq!(expected_length, actual_length);
        }
    }

    #[cfg(test)]
    mod http_1x_tests {
        use crate::content_encoding::FIXED_CHUNK_SIZE_BYTE;

        use super::super::{
            http_1x_utils::{total_rendered_length_of_trailers, trailers_as_aws_chunked_bytes},
            AwsChunkedBody, AwsChunkedBodyOptions, CHUNK_TERMINATOR_RAW, CRLF_RAW,
        };

        use aws_smithy_types::body::SdkBody;
        use bytes::{Buf, Bytes, BytesMut};
        use bytes_utils::SegmentedBuf;
        use http_1x::{HeaderMap, HeaderValue};
        use http_body_1x::{Body, Frame, SizeHint};
        use http_body_util::BodyExt;
        use pin_project_lite::pin_project;

        use std::io::Read;
        use std::pin::Pin;
        use std::task::{Context, Poll};
        use std::time::Duration;

        pin_project! {
            struct SputteringBody {
                parts: Vec<Option<Bytes>>,
                cursor: usize,
                delay_in_millis: u64,
            }
        }

        impl SputteringBody {
            fn len(&self) -> usize {
                self.parts.iter().flatten().map(|b| b.len()).sum()
            }
        }

        impl Body for SputteringBody {
            type Data = Bytes;
            type Error = aws_smithy_types::body::Error;

            fn poll_frame(
                self: Pin<&mut Self>,
                cx: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
                if self.cursor == self.parts.len() {
                    return Poll::Ready(None);
                }

                let this = self.project();
                let delay_in_millis = *this.delay_in_millis;
                let next_part = this.parts.get_mut(*this.cursor).unwrap().take();

                match next_part {
                    None => {
                        *this.cursor += 1;
                        let waker = cx.waker().clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(Duration::from_millis(delay_in_millis)).await;
                            waker.wake();
                        });
                        Poll::Pending
                    }
                    Some(data) => {
                        *this.cursor += 1;
                        let frame = Frame::data(data);
                        Poll::Ready(Some(Ok(frame)))
                    }
                }
            }

            fn is_end_stream(&self) -> bool {
                false
            }

            fn size_hint(&self) -> SizeHint {
                SizeHint::new()
            }
        }

        // Custom body that returns data and trailers
        pin_project! {
            struct TestBodyWithTrailers {
                data: Option<Bytes>,
                trailers: Option<HeaderMap>,
            }
        }

        impl Body for TestBodyWithTrailers {
            type Data = Bytes;
            type Error = aws_smithy_types::body::Error;

            fn poll_frame(
                self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
            ) -> Poll<Option<Result<http_body_1x::Frame<Self::Data>, Self::Error>>> {
                let this = self.project();

                if let Some(data) = this.data.take() {
                    return Poll::Ready(Some(Ok(http_body_1x::Frame::data(data))));
                }

                if let Some(trailers) = this.trailers.take() {
                    return Poll::Ready(Some(Ok(http_body_1x::Frame::trailers(trailers))));
                }

                Poll::Ready(None)
            }
        }

        #[tokio::test]
        async fn test_aws_chunked_encoding() {
            let test_fut = async {
                let input_str = "Hello world";
                let opts = AwsChunkedBodyOptions::new(input_str.len() as u64, vec![]);
                let mut body = AwsChunkedBody::new(SdkBody::from(input_str), opts);

                let mut output = SegmentedBuf::new();
                while let Some(Ok(buf)) = body.frame().await {
                    output.push(buf.into_data().unwrap());
                }

                let mut actual_output = String::new();
                output
                    .reader()
                    .read_to_string(&mut actual_output)
                    .expect("Doesn't cause IO errors");

                let expected_output = "B\r\nHello world\r\n0\r\n\r\n";

                assert_eq!(expected_output, actual_output);

                // You can insert a `tokio::time::sleep` here to verify the timeout works as intended
            };

            let timeout_duration = Duration::from_secs(3);
            if tokio::time::timeout(timeout_duration, test_fut)
                .await
                .is_err()
            {
                panic!("test_aws_chunked_encoding timed out after {timeout_duration:?}");
            }
        }

        #[tokio::test]
        async fn test_aws_chunked_encoding_sputtering_body() {
            let test_fut = async {
                let input = SputteringBody {
                    parts: vec![
                        Some(Bytes::from_static(b"chunk 1, ")),
                        None,
                        Some(Bytes::from_static(b"chunk 2, ")),
                        Some(Bytes::from_static(b"chunk 3, ")),
                        None,
                        None,
                        Some(Bytes::from_static(b"chunk 4, ")),
                        Some(Bytes::from_static(b"chunk 5, ")),
                        Some(Bytes::from_static(b"chunk 6")),
                    ],
                    cursor: 0,
                    delay_in_millis: 500,
                };
                let opts = AwsChunkedBodyOptions::new(input.len() as u64, vec![]);
                let mut body = AwsChunkedBody::new(input, opts);

                let mut output = SegmentedBuf::new();
                while let Some(Ok(buf)) = body.frame().await {
                    output.push(buf.into_data().unwrap());
                }

                let mut actual_output = String::new();
                output
                    .reader()
                    .read_to_string(&mut actual_output)
                    .expect("Doesn't cause IO errors");

                let expected_output =
                    "34\r\nchunk 1, chunk 2, chunk 3, chunk 4, chunk 5, chunk 6\r\n0\r\n\r\n";

                assert_eq!(expected_output, actual_output);
            };

            let timeout_duration = Duration::from_secs(3);
            if tokio::time::timeout(timeout_duration, test_fut)
                .await
                .is_err()
            {
                panic!(
                "test_aws_chunked_encoding_sputtering_body timed out after {timeout_duration:?}"
            );
            }
        }

        #[tokio::test]
        async fn test_aws_chunked_encoding_incorrect_trailer_length_panic() {
            let input_str = "Hello world";
            // Test body has no trailers, so this length is incorrect and will trigger an assert panic
            // When the panic occurs, it will actually expect a length of 44. This is because, when using
            // aws-chunked encoding, each trailer will end with a CRLF which is 2 bytes long.
            let wrong_trailer_len = 42;
            let opts = AwsChunkedBodyOptions::new(input_str.len() as u64, vec![wrong_trailer_len]);
            let mut body = AwsChunkedBody::new(SdkBody::from(input_str), opts);

            // We don't care about the body contents but we have to read it all before checking for trailers
            while let Some(Ok(frame)) = body.frame().await {
                assert!(!frame.is_trailers());
            }
        }

        #[tokio::test]
        async fn test_aws_chunked_encoding_empty_body() {
            let input_str = "";
            let opts = AwsChunkedBodyOptions::new(input_str.len() as u64, vec![]);
            let mut body = AwsChunkedBody::new(SdkBody::from(input_str), opts);

            let mut output = SegmentedBuf::new();
            while let Some(Ok(frame)) = body.frame().await {
                output.push(frame.into_data().unwrap());
            }

            let mut actual_output = String::new();
            output
                .reader()
                .read_to_string(&mut actual_output)
                .expect("Doesn't cause IO errors");

            let actual_output = std::str::from_utf8(actual_output.as_bytes()).unwrap();
            let expected_output = [CHUNK_TERMINATOR_RAW, CRLF_RAW].concat();
            let expected_output = std::str::from_utf8(&expected_output).unwrap();

            assert_eq!(expected_output, actual_output);
        }

        #[tokio::test]
        async fn test_total_rendered_length_of_trailers() {
            let mut trailers = HeaderMap::new();

            trailers.insert("empty_value", HeaderValue::from_static(""));

            trailers.insert("single_value", HeaderValue::from_static("value 1"));

            trailers.insert("two_values", HeaderValue::from_static("value 1"));
            trailers.append("two_values", HeaderValue::from_static("value 2"));

            trailers.insert("three_values", HeaderValue::from_static("value 1"));
            trailers.append("three_values", HeaderValue::from_static("value 2"));
            trailers.append("three_values", HeaderValue::from_static("value 3"));

            let trailers = Some(&trailers);
            let actual_length = total_rendered_length_of_trailers(trailers);
            let buf = BytesMut::with_capacity(actual_length as usize);
            let expected_length = (trailers_as_aws_chunked_bytes(trailers, buf).len()) as u64;

            assert_eq!(expected_length, actual_length);
        }

        #[tokio::test]
        async fn test_total_rendered_length_of_empty_trailers() {
            let header_map = HeaderMap::new();
            let trailers = Some(&header_map);
            let actual_length = total_rendered_length_of_trailers(trailers);
            let buf = BytesMut::with_capacity(actual_length as usize);
            let expected_length = (trailers_as_aws_chunked_bytes(trailers, buf).len()) as u64;

            assert_eq!(expected_length, actual_length);
        }

        #[tokio::test]
        async fn test_poll_frame_with_default_chunk_size() {
            let test_data = Bytes::from("1234567890123456789012345");
            let body = SdkBody::from(test_data.clone());
            let options = AwsChunkedBodyOptions::new(test_data.len() as u64, vec![]);
            let mut chunked_body = AwsChunkedBody::new(body, options);

            let mut data_frames = Vec::new();
            while let Some(frame) = chunked_body.frame().await.transpose().unwrap() {
                if let Ok(data) = frame.into_data() {
                    data_frames.push(data);
                }
            }

            assert_eq!(data_frames.len(), 2); // Data fits in one chunk, plus the final chunk
            assert_eq!(
                Bytes::from_static(b"19\r\n1234567890123456789012345\r\n"),
                data_frames[0]
            );
            assert_eq!(Bytes::from_static(b"0\r\n\r\n"), data_frames[1]);
        }

        #[tokio::test]
        async fn test_poll_frame_with_custom_chunk_size() {
            let test_data = Bytes::from("1234567890123456789012345");
            let body = SdkBody::from(test_data.clone());
            let options =
                AwsChunkedBodyOptions::new(test_data.len() as u64, vec![]).with_chunk_size(10);
            let mut chunked_body = AwsChunkedBody::new(body, options);

            let mut data_frames = Vec::new();
            while let Some(frame) = chunked_body.frame().await.transpose().unwrap() {
                if let Ok(data) = frame.into_data() {
                    data_frames.push(data);
                }
            }

            assert_eq!(4, data_frames.len()); // 25 bytes / 10 = 2.5 so 3 chunks, plus the final chunk
            assert_eq!(Bytes::from_static(b"A\r\n1234567890\r\n"), data_frames[0]);
            assert_eq!(Bytes::from_static(b"A\r\n1234567890\r\n"), data_frames[1]);
            assert_eq!(Bytes::from_static(b"5\r\n12345\r\n"), data_frames[2]);
            assert_eq!(Bytes::from_static(b"0\r\n\r\n"), data_frames[3]);
        }

        #[tokio::test]
        async fn test_poll_frame_with_trailers() {
            let data = Bytes::from("1234567890123456789012345");
            let stream_len = data.len() as u64;
            let mut trailers = HeaderMap::new();
            trailers.insert("x-amz-checksum-crc32", HeaderValue::from_static("78DeVw=="));
            let body = TestBodyWithTrailers {
                data: Some(data),
                trailers: Some(trailers),
            };
            let options = AwsChunkedBodyOptions::new(stream_len, vec![29]).with_chunk_size(10);
            let mut chunked_body = AwsChunkedBody::new(body, options);

            let mut data_frames = Vec::new();
            while let Some(frame) = chunked_body.frame().await.transpose().unwrap() {
                if let Ok(data) = frame.into_data() {
                    data_frames.push(data);
                }
            }

            assert_eq!(4, data_frames.len()); // 25 bytes / 10 = 2.5 so 3 chunks, plus the final chunk
            assert_eq!(Bytes::from_static(b"A\r\n1234567890\r\n"), data_frames[0]);
            assert_eq!(Bytes::from_static(b"A\r\n1234567890\r\n"), data_frames[1]);
            assert_eq!(Bytes::from_static(b"5\r\n12345\r\n"), data_frames[2]);
            assert_eq!(
                Bytes::from_static(b"0\r\nx-amz-checksum-crc32:78DeVw==\r\n\r\n"),
                data_frames[3]
            );
        }

        // Testing scenario derived from https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-streaming.html
        #[tokio::test]
        async fn test_aws_chunked_body_poll_frame_with_signer() {
            use crate::auth::sigv4::SigV4MessageSigner;
            use aws_credential_types::Credentials;
            use aws_sigv4::http_request::SigningSettings;
            use aws_smithy_async::time::{SharedTimeSource, StaticTimeSource};
            use aws_types::region::SigningRegion;
            use aws_types::SigningName;
            use std::time::{Duration, UNIX_EPOCH};

            // 65KB of 'a' characters
            let data = "a".repeat(65 * 1024);
            let stream_len = data.len() as u64;
            let inner_body = SdkBody::from(data);

            // `StaticTimeSource` for 20130524T000000Z
            let time = StaticTimeSource::new(UNIX_EPOCH + Duration::from_secs(1369353600));
            let shared_time = SharedTimeSource::from(time);

            let credentials = Credentials::new(
                "AKIAIOSFODNN7EXAMPLE",
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                None,
                None,
                "test",
            );

            let seed_signature =
                "4f232c4386841ef735655705268965c44a0e4690baa4adea153f7db9fa80a0a9".to_owned();
            let signer = SigV4MessageSigner::new(
                seed_signature,
                credentials.into(),
                SigningRegion::from_static("us-east-1"),
                SigningName::from_static("s3"),
                shared_time,
                SigningSettings::default(),
            );

            let opt = AwsChunkedBodyOptions::new(stream_len, vec![]).signed_chunked_encoding(true);
            let mut chunked_body = AwsChunkedBody::new(inner_body, opt).with_signer(signer);

            let mut data_frames = Vec::new();
            while let Some(frame) = chunked_body.frame().await.transpose().unwrap() {
                if let Ok(data) = frame.into_data() {
                    data_frames.push(data);
                }
            }

            assert_eq!(3, data_frames.len()); // 64 KB, 1 KB, and the final chunk with 0 bytes of chunk data.
            assert!(data_frames[0].starts_with(b"10000;chunk-signature=ad80c730a21e5b8d04586a2213dd63b9a0e99e0e2307b0ade35a65485a288648\r\n"));
            assert!(data_frames[1].starts_with(b"400;chunk-signature=0055627c9e194cb4542bae2aa5492e3c1575bbb81b612b7d234b86a503ef5497\r\n"));
            assert_eq!(data_frames[2], Bytes::from_static(b"0;chunk-signature=b6c6ea8a5354eaf15b3cb7646744f4275b71ea724fed81ceb9323e279d449df9\r\n\r\n"));
        }

        // Testing scenario derived from https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-streaming-trailers.html
        #[tokio::test]
        async fn test_aws_chunked_body_poll_frame_with_signer_and_trailers() {
            use crate::auth::sigv4::SigV4MessageSigner;
            use aws_credential_types::Credentials;
            use aws_sigv4::http_request::SigningSettings;
            use aws_smithy_async::time::{SharedTimeSource, StaticTimeSource};
            use aws_types::region::SigningRegion;
            use aws_types::SigningName;
            use std::time::{Duration, UNIX_EPOCH};

            // 65KB of 'a' characters
            let data = "a".repeat(65 * 1024);
            let stream_len = data.len() as u64;

            // Set trailers with x-amz-checksum-crc32c header
            let mut trailers = HeaderMap::new();
            trailers.insert(
                "x-amz-checksum-crc32c",
                HeaderValue::from_static("sOO8/Q=="),
            );

            let inner_body = TestBodyWithTrailers {
                data: Some(Bytes::from(data)),
                trailers: Some(trailers),
            };

            // `StaticTimeSource` for 20130524T000000Z
            let time = StaticTimeSource::new(UNIX_EPOCH + Duration::from_secs(1369353600));
            let shared_time = SharedTimeSource::from(time);

            let credentials = Credentials::new(
                "AKIAIOSFODNN7EXAMPLE",
                "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
                None,
                None,
                "test",
            );

            let seed_signature =
                "106e2a8a18243abcf37539882f36619c00e2dfc72633413f02d3b74544bfeb8e".to_owned();
            let signer = SigV4MessageSigner::new(
                seed_signature,
                credentials.into(),
                SigningRegion::from_static("us-east-1"),
                SigningName::from_static("s3"),
                shared_time,
                SigningSettings::default(),
            );

            let opt =
                AwsChunkedBodyOptions::new(stream_len, vec![30, 88]).signed_chunked_encoding(true);
            let mut chunked_body = AwsChunkedBody::new(inner_body, opt).with_signer(signer);

            let mut data_frames = Vec::new();
            while let Some(frame) = chunked_body.frame().await.transpose().unwrap() {
                if let Ok(data) = frame.into_data() {
                    data_frames.push(data);
                }
            }

            assert_eq!(4, data_frames.len()); // 64 KB, 1 KB, 0 bytes of chunk data, and the trailer chunk.
            assert!(data_frames[0].starts_with(b"10000;chunk-signature=b474d8862b1487a5145d686f57f013e54db672cee1c953b3010fb58501ef5aa2\r\n"));
            assert!(data_frames[1].starts_with(b"400;chunk-signature=1c1344b170168f8e65b41376b44b20fe354e373826ccbbe2c1d40a8cae51e5c7\r\n"));
            assert_eq!(data_frames[2], Bytes::from_static(b"0;chunk-signature=2ca2aba2005185cf7159c6277faf83795951dd77a3a99e6e65d5c9f85863f992\r\n"));
            assert_eq!(data_frames[3], Bytes::from_static(b"x-amz-checksum-crc32c:sOO8/Q==\r\nx-amz-trailer-signature:d81f82fc3505edab99d459891051a732e8730629a2e4a59689829ca17fe2e435\r\n\r\n"));
        }

        #[test]
        fn test_unsigned_encoded_length_with_no_trailer() {
            {
                let options = AwsChunkedBodyOptions::new(10, vec![]);
                /*
                 A\r\n
                 10 bytes of data\r\n
                 0\r\n
                 \r\n
                 -------------------------------------------------------------
                 1 (A) + 2 (\r\n) +
                 10 (data) + 2 (\r\n) +
                 1 (0) + 2 (\r\n) +
                 2 (\r\n)

                    = 20 total bytes
                */
                assert_eq!(options.encoded_length(), 20);
            }
            {
                let options =
                    AwsChunkedBodyOptions::new((FIXED_CHUNK_SIZE_BYTE + 10) as u64, vec![]);
                /*
                 10000\r\n
                 65536 bytes of data\r\n
                 A\r\n
                 10 bytes of data\r\n
                 0\r\n
                 \r\n
                 -------------------------------------------------------------
                 5 (10000) + 2 (\r\n) +
                 65536 (data) + 2 (\r\n) +
                 1 (A) + 2 (\r\n) +
                 10 (data) + 2 (\r\n) +
                 1 (0) + 2 (\r\n) +
                 2 (\r\n)

                    = 65565 total bytes
                */
                assert_eq!(options.encoded_length(), 65565);
            }
        }

        #[test]
        fn test_unsigned_encoded_length_with_trailer() {
            let options = AwsChunkedBodyOptions::new(10, vec![30]);
            /*
                A\r\n
                10 bytes of data\r\n
                0\r\n
                x-amz-checksum-crc32c:sOO8/Q==\r\n
                \r\n
                -------------------------------------------------------------
                1 (A) + 2 (\r\n) +
                10 (data) + 2 (\r\n) +
                1 (0) + 2 (\r\n) +
                21 (x-amz-checksum-crc32c) + 1 (:) + 8 (sOO8/Q==) + 2 (\r\n) +
                2 (\r\n)

                    = 52 total bytes
            */
            assert_eq!(options.encoded_length(), 52);
        }

        #[test]
        fn test_signed_encoded_length_with_no_trailer() {
            {
                let options = AwsChunkedBodyOptions::new(10, vec![]).signed_chunked_encoding(true);
                /*
                 A;chunk-signature=<signature>\r\n
                 10 bytes of data\r\n
                 0;chunk-signature=<signature>\r\n
                 \r\n
                 -------------------------------------------------------------
                 1 (A) + 17 (;chunk-signature=) + 64 (signature) + 2 (\r\n) +
                 10 (data) + 2 (\r\n) +
                 1 (0) + 17 (;chunk-signature) + 64 (signature) + 2 (\r\n) +
                 2 (\r\n)

                    = 182 total bytes
                */
                assert_eq!(options.encoded_length(), 182);
            }
            {
                let options =
                    AwsChunkedBodyOptions::new((FIXED_CHUNK_SIZE_BYTE + 10) as u64, vec![])
                        .signed_chunked_encoding(true);
                /*
                 10000;chunk-signature=<signature>\r\n
                 65536 bytes of data\r\n
                 A;chunk-signature=<signature>\r\n
                 10 bytes of data\r\n
                 0;chunk-signature=<signature>\r\n
                 \r\n
                 -------------------------------------------------------------
                 5 (10000) + 17 (;chunk-signature=) + 64 (signature) + 2 (\r\n) +
                 65536 (data) + 2 (\r\n) +
                 1 (A) + 17 (;chunk-signature=) + 64 (signature) + 2 (\r\n) +
                 10 (data) + 2 (\r\n) +
                 1 (0) + 17 (;chunk-signature) + 64 (signature) + 2 (\r\n) +
                 2 (\r\n)

                    = 65808 total bytes
                */
                assert_eq!(options.encoded_length(), 65808);
            }
        }

        #[test]
        fn test_signed_encoded_length_with_trailer() {
            let options =
                AwsChunkedBodyOptions::new(10, vec![30, 88]).signed_chunked_encoding(true);
            /*
                A;chunk-signature=<signature>\r\n
                10 bytes of data\r\n
                0;chunk-signature=<signature>\r\n
                x-amz-checksum-crc32c:sOO8/Q==\r\n
                x-amz-trailer-signature:<signature>\r\n
                \r\n
                -------------------------------------------------------------
                1 (A) + 17 (;chunk-signature=) + 64 (signature) + 2 (\r\n) +
                10 (data) + 2 (\r\n) +
                1 (0) + 17 (;chunk-signature) + 64 (signature) + 2 (\r\n) +
                21 (x-amz-checksum-crc32c) + 1 (:) + 8 (sOO8/Q==) + 2 (\r\n) +
                23 (x-amz-trailer-signature) + 1 (:) + 64 (signature) + 2 (\r\n) +
                2 (\r\n)

                    = 304 total bytes
            */
            assert_eq!(options.encoded_length(), 304);
        }
    }
}
