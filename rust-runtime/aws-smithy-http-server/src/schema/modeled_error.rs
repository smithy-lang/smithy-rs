/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use aws_smithy_schema::serde::SerializableStruct;

/// A modeled `@error` shape that carries its own schema.
///
/// The schema returned by [`SerializableStruct::schema`] must carry the Smithy `@error`
/// trait. It supplies the members, HTTP bindings, and shape ID used by server protocols.
pub trait ModeledError: SerializableStruct {}

/// A modeled error that a [`ServerProtocol`](super::ServerProtocol) can turn into an HTTP response.
pub trait HttpModeledError: ModeledError + std::error::Error + Send + Sync + 'static {
    /// The HTTP status code for this error: `@httpError` when present, otherwise `400` for
    /// `@error("client")` shapes and `500` for `@error("server")` shapes.
    fn status_code(&self) -> u16;
}
