/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */
//! Serialize HTTP response and error traits

/// `SerializeHttpResponse` is a generic trait for serializing structured data into HTTP responses.
///
/// It is designed to be flexible, because `Output` and `Struct` are unconstrained, it can be used to support
/// event streams, regular request-response style operations, as well as any other HTTP-based
/// protocol that we manage to come up with.
///
/// It also enables this critical and core trait to avoid being async, and it makes code that uses
/// the trait easier to test.
///
/// TODO: streaming and not-fully loaded body serialization is no developed yet
pub trait SerializeHttpResponse {
    /// Struct instance to be serialized into the HTTP body
    ///
    /// For request/response style operations, this is typically something like:
    /// `OperationOutput`
    type Struct;
    /// HTTP response output
    ///
    /// For request/response style operations, this is typically something like:
    /// `Result<ResponseBytes>, Error>`
    type Output;

    /// Serialize an HTTP response from a fully loaded body. This is for standard request/response style
    /// APIs like RestJson1.
    fn serialize(&self, output: &Self::Struct) -> Self::Output;
}

/// `SerializeHttpError` is a generic trait for serializing structured errors into HTTP responses.
///
/// It is designed to be flexible, because `Output` and `Struct` are unconstrained, it can be used to support
/// event streams, regular request-response style operations, as well as any other HTTP-based
/// protocol that we manage to come up with.
///
/// It also enables this critical and core trait to avoid being async, and it makes code that uses
/// the trait easier to test.
///
/// TODO: streaming and not-fully loaded body serialization is no developed yet
pub trait SerializeHttpError {
    /// Struct instance to be serialized into the HTTP body
    ///
    /// For request/response style operations, this is typically something like:
    /// `OperationError`
    type Struct;
    /// HTTP response output
    ///
    /// For request/response style operations, this is typically something like:
    /// `Result<ResponseBytes>, Error>`
    type Output;

    /// Serialize an HTTP response from a fully loaded body. This is for standard request/response style
    /// APIs like RestJson1.
    fn serialize(&self, error: &Self::Struct) -> Self::Output;
}

#[cfg(test)]
mod test {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn support_non_streaming_body() {
        pub struct S3GetObject {
            pub body: Bytes,
        }

        #[allow(dead_code)]
        pub enum S3Error {
            Something,
        }

        impl S3Error {
            fn as_bytes(&self) -> Bytes {
                match self {
                    S3Error::Something => Bytes::from_static(b"something"),
                }
            }
        }

        struct S3GetObjectParser;

        impl SerializeHttpResponse for S3GetObjectParser {
            type Struct = S3GetObject;
            type Output = http::Response<bytes::Bytes>;

            fn serialize(&self, output: &Self::Struct) -> Self::Output {
                http::Response::builder().body(output.body.clone()).unwrap()
            }
        }

        impl SerializeHttpError for S3GetObjectParser {
            type Struct = S3Error;
            type Output = http::Response<bytes::Bytes>;

            fn serialize(&self, error: &Self::Struct) -> Self::Output {
                http::Response::builder().body(error.as_bytes()).unwrap()
            }
        }
    }
}
