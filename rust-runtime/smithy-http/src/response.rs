/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use bytes::Bytes;
use http::Response;

/// `ParseHttpResponse` is a generic trait for parsing structured data from HTTP respsones.
///
/// It is designed to be nearly infinitely flexible—because Because `Output` is unstrained, it can be used to support
/// event streams, S3 streaming responses, regular request-response style operations, as well
/// as any other HTTP-based protocol that we manage to come up with.
///
/// The split between `parse_unloaded` and `parse_loaded` enables keeping the parsing code pure and sync
/// whenever possible and delegating the process of actually reading the HTTP response to the caller when
/// the required behavior is simply "read to the end."
///
/// It also enables this critical and core trait to avoid being async, and it makes code that uses
/// the trait easier to test.
pub trait ParseHttpResponse<B> {
    /// Output type of the HttpResponse.
    ///
    /// For request/response style operations, this is typically something like:
    /// `Result<ListTablesResponse, ListTablesError>`
    ///
    /// For streaming operations, this is something like:
    /// `Result<EventStream<TranscribeStreamingEvent>, TranscribeStreamingError>`
    type Output;

    /// Parse an HTTP request without reading the body. If the body must be provided to proceed,
    /// return `None`
    ///
    /// This exists to serve APIs like S3::GetObject where the body is passed directly into the
    /// response and consumed by the client. However, even in the case of S3::GetObject, errors
    /// require reading the entire body.
    ///
    /// This also facilitates `EventStream` and other streaming HTTP protocols by enabling the
    /// handler to take ownership of the HTTP response directly.
    ///
    /// Currently `parse_unloaded` operates on a borrowed HTTP request to enable
    /// the caller to provide a raw HTTP response to the caller for inspection after the response is
    /// returned. For EventStream-like use cases, the caller can use `mem::swap` to replace
    /// the streaming body with an empty body as long as the body implements default.
    ///
    /// We should consider if this is too limiting & if this should take an owned response instead.
    fn parse_unloaded(&self, response: &mut http::Response<B>) -> Option<Self::Output>;

    /// Parse an HTTP request from a fully loaded body. This is for standard request/response style
    /// APIs like AwsJSON as well as for the error path of most streaming APIs
    ///
    /// Using an explicit body type of Bytes here is a conscious decision—If you _really_ need
    /// to precisely control how the data is loaded into memory, use `parse_unloaded`.
    fn parse_loaded(&self, response: &http::Response<Bytes>) -> Self::Output;
}

/// Convenience Trait for non-streaming APIs
///
/// `ParseStrictResponse` enables operations that _never_ need to stream the body incrementally to
/// have cleaner implementations. There is a blanket implementation
pub trait ParseStrictResponse {
    type Output;
    fn parse(&self, response: &Response<Bytes>) -> Self::Output;
}

impl<B, T> ParseHttpResponse<B> for T
where
    T: ParseStrictResponse,
{
    type Output = T::Output;

    fn parse_unloaded(&self, _response: &mut Response<B>) -> Option<Self::Output> {
        None
    }

    fn parse_loaded(&self, response: &Response<Bytes>) -> Self::Output {
        self.parse(response)
    }
}
