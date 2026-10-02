/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::SerializableStruct;
use aws_smithy_schema::Schema;

/// A modeled error that a [`ServerProtocol`](super::ServerProtocol) can turn into an HTTP response.
pub trait HttpModeledError: SerializableStruct + std::error::Error + Send + Sync + 'static {
    /// Returns the schema of the active modeled error shape.
    fn schema(&self) -> &Schema<'_>;

    /// The HTTP status code for this error: `@httpError` when present, otherwise `400` for
    /// `@error("client")` shapes and `500` for `@error("server")` shapes.
    fn status_code(&self) -> u16;
}

// Operations without modeled errors use Infallible. These methods can never be called.
impl HttpModeledError for std::convert::Infallible {
    fn schema(&self) -> &Schema<'_> {
        match *self {}
    }

    fn status_code(&self) -> u16 {
        match *self {}
    }
}
