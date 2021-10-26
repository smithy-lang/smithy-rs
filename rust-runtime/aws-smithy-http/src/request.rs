/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

use bytes::Bytes;

/// `ParseHttpResponse` is a generic trait for parsing structured data from HTTP responses.
///
/// It is designed to be nearly infinitely flexible, because `Output` is unconstrained, it can be used to support
/// event streams, S3 streaming responses, regular request-response style operations, as well
/// as any other HTTP-based protocol that we manage to come up with.
///
/// The split between `parse_unloaded` and `parse_loaded` enables keeping the parsing code pure and sync
/// whenever possible and delegating the process of actually reading the HTTP response to the caller when
/// the required behavior is simply "read to the end."
///
/// It also enables this critical and core trait to avoid being async, and it makes code that uses
/// the trait easier to test.
pub trait ParseHttpRequest {
    /// Output type of the HttpResponse.
    ///
    /// For request/response style operations, this is typically something like:
    /// `Result<ListTablesResponse, ListTablesError>`
    ///
    /// For streaming operations, this is something like:
    /// `Result<EventStream<TranscribeStreamingEvent>, TranscribeStreamingError>`
    type Input;

    /// Parse an HTTP request from a fully loaded body. This is for standard request/response style
    /// APIs like AwsJson 1.0/1.1 and the error path of most streaming APIs
    ///
    /// Using an explicit body type of Bytes here is a conscious decision—If you _really_ need
    /// to precisely control how the data is loaded into memory (eg. by using `bytes::Buf`), implement
    /// your handler in `parse_unloaded`.
    ///
    /// Production code will never call `parse_loaded` without first calling `parse_unloaded`. However,
    /// in tests it may be easier to use `parse_loaded` directly. It is OK to panic in `parse_loaded`
    /// if `parse_unloaded` will never return `None`, however, it may make your code easier to test if an
    /// implementation is provided.
    fn parse_loaded(&self, request: &http::Request<Bytes>) -> Self::Input;
}

/// Convenience Trait for non-streaming APIs
///
/// `ParseStrictResponse` enables operations that _never_ need to stream the body incrementally to
/// have cleaner implementations. There is a blanket implementation
pub trait ParseStrictRequest {
    type Input;
    fn parse(&self, request: &http::Request<Bytes>) -> Self::Input;
}

impl<T: ParseStrictRequest> ParseHttpRequest for T {
    type Input = T::Input;

    fn parse_loaded(&self, request: &http::Request<Bytes>) -> Self::Input {
        self.parse(request)
    }
}

#[cfg(test)]
mod test {
    use crate::body::SdkBody;
    use crate::operation;
    use crate::response::ParseHttpResponse;
    use bytes::Bytes;
    use std::mem;

    #[test]
    fn supports_streaming_body() {}
}
