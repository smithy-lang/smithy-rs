/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

//! Parse HTTP request trait

use aws_smithy_http::body::SdkBody;
use bytes::Bytes;

/// `ParseHttpRequest` is a generic trait for parsing structured data from HTTP requests.
///
/// It is designed to be flexible, because `Input` is unconstrained, it can be used to support
/// event streams, regular request-response style operations, as well as any other HTTP-based
/// protocol that we manage to come up with.
///
/// It also enables this critical and core trait to avoid being async, and it makes code that uses
/// the trait easier to test.
pub trait ParseHttpRequest {
    /// Input type of the HttpRequest
    ///
    /// For request/response style operations, this is typically something like:
    /// `Result<OperationInput, OperationInputError>`
    ///
    /// For streaming operations, this is something like:
    /// `Result<EventStream<OperationEvent>>, OperationStreamingError>`
    type Input;

    /// Parse an HTTP request without reading the body. If the body must be provided to proceed,
    /// return `None`
    ///
    /// This exists to serve APIs like S3::GetObject where the body is passed directly into the
    /// response and consumed by the client. However, even in the case of S3::GetObject, errors
    /// require reading the entire body.
    ///
    /// This also facilitates `EventStream` and other streaming HTTP protocols by enabling the
    /// handler to take ownership of the HTTP request directly.
    ///
    /// Currently `parse_unloaded` operates on a borrowed HTTP request to enable
    /// the caller to provide a raw HTTP response to the caller for inspection after the response is
    /// returned. For EventStream-like use cases, the caller can use `mem::swap` to replace
    /// the streaming body with an empty body as long as the body implements default.
    ///
    /// We should consider if this is too limiting & if this should take an owned response instead.
    fn parse_unloaded(&self, request: &mut http::Request<SdkBody>) -> Option<Self::Input>;

    /// Parse an HTTP request from a fully loaded body. This is for standard request/response style
    /// APIs like RestJson1.
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

#[cfg(test)]
mod test {
    use super::*;
    use bytes::Bytes;
    use std::mem;

    #[test]
    fn support_non_streaming_body() {
        pub struct S3GetObject {
            pub body: Bytes,
        }

        struct S3GetObjectParser;

        impl ParseHttpRequest for S3GetObjectParser {
            type Input = S3GetObject;

            fn parse_unloaded(&self, _request: &mut http::Request<SdkBody>) -> Option<Self::Input> {
                None
            }

            fn parse_loaded(&self, request: &http::Request<Bytes>) -> Self::Input {
                S3GetObject {
                    body: request.body().clone(),
                }
            }
        }
    }
    #[test]
    fn supports_streaming_body() {
        pub struct S3GetObject {
            pub body: SdkBody,
        }

        struct S3GetObjectParser;

        impl ParseHttpRequest for S3GetObjectParser {
            type Input = S3GetObject;

            fn parse_unloaded(&self, request: &mut http::Request<SdkBody>) -> Option<Self::Input> {
                // For responses that pass on the body, use mem::take to leave behind an empty body
                let body = mem::replace(request.body_mut(), SdkBody::taken());
                Some(S3GetObject { body })
            }

            fn parse_loaded(&self, _request: &http::Request<Bytes>) -> Self::Input {
                unimplemented!()
            }
        }
    }
}
