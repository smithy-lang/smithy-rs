/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::SerializableStruct;
use aws_smithy_schema::Schema;

/// A modeled `@error` shape that carries its own schema.
///
/// [`SerializableStruct`] alone cannot say which shape it serializes; the schema supplies the
/// members, the HTTP bindings and the shape ID that protocols use as the error discriminator.
pub trait ModeledError: SerializableStruct {
    /// The schema of this error shape.
    fn schema(&self) -> &Schema<'_>;
}

/// A modeled error that a [`ServerProtocol`](super::ServerProtocol) can turn into an HTTP response.
pub trait HttpModeledError: ModeledError + std::error::Error + Send + Sync + 'static {
    /// The HTTP status code for this error: `@httpError` when present, otherwise `400` for
    /// `@error("client")` shapes and `500` for `@error("server")` shapes.
    fn status_code(&self) -> u16;
}
