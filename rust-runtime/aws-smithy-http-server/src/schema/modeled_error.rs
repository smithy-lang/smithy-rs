/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::SerializableStruct;

/// A modeled error that a [`ServerProtocol`](super::ServerProtocol) can turn into an HTTP response.
pub trait HttpModeledError: SerializableStruct + std::error::Error + Send + Sync + 'static {
    /// The HTTP status code for this error: `@httpError` when present, otherwise `400` for
    /// `@error("client")` shapes and `500` for `@error("server")` shapes.
    fn status_code(&self) -> u16;

    /// The operation that returned this error, when known.
    ///
    /// Generated operation error enums expose their operation's input, output, and
    /// error schemas. Shared error structures and errors created outside an operation
    /// have no operation context. Each protocol decides whether to use this context.
    fn operation_schema(&self) -> Option<&super::OperationSchema<'_>> {
        None
    }
}

// Operations without modeled errors use Infallible. These methods can never be called.
impl HttpModeledError for std::convert::Infallible {
    fn status_code(&self) -> u16 {
        match *self {}
    }
}
