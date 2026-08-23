/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Modeled-error abstraction: the traits implemented by every `@error` shape.
//!
//! Every error that reaches the wire is a *modeled error* — a structure carrying
//! the `@error` trait in the Smithy model, serialized through the schema-driven
//! codecs. There are no hand-rolled error serializers.
//!
//! Two traits form the abstraction:
//!
//! - [`ModeledError`]: protocol-agnostic marker implemented by every `@error`
//!   structure. It supplies the shape's [`Schema`] (absent from
//!   `aws-smithy-schema`'s [`SerializableStruct`]).
//! - [`HttpModeledError`]: HTTP extension adding [`status_code`](HttpModeledError::status_code).
//!   This is the bound accepted by
//!   [`ServerProtocol::serialize_error`](crate::protocol::server_protocol::ServerProtocol::serialize_error).
//!
//! Generated code implements both for every `@error` structure (service,
//! framework, and middleware models alike), returning the shape's
//! `Schema<'static>` const and baking the resolved HTTP status as a literal
//! (`@httpError` code, else the `@error` fault default — client = 400,
//! server = 500).
//!
//! Event-stream-only error shapes implement [`ModeledError`] but *not*
//! [`HttpModeledError`] — a status code is meaningless mid-stream. Shapes used
//! both as operation errors and event-stream errors implement both. (Note:
//! under current codegen every event-stream error is *also* hoisted into the
//! operation error enum, so in practice the `ModeledError`-only bucket is
//! empty today.)

use aws_smithy_schema::serde::SerializableStruct;
use aws_smithy_schema::Schema;

/// Marker: this shape is a modeled `@error` structure. Protocol-agnostic.
///
/// Supplies the schema (absent from `aws-smithy-schema`'s
/// [`SerializableStruct`]).
pub trait ModeledError: SerializableStruct {
    /// Returns the schema for this error shape.
    ///
    /// Generated implementations return the shape's `Schema<'static>` const.
    fn schema(&self) -> &Schema<'_>;
}

/// HTTP extension of [`ModeledError`]; the bound accepted by
/// `aws-smithy-http-server`'s error serialization seam.
pub trait HttpModeledError: ModeledError {
    /// The HTTP status code for this error.
    ///
    /// Generated implementations bake a literal resolved at codegen time:
    /// the `@httpError` code if present, else the `@error` fault default
    /// (client = 400, server = 500).
    fn status_code(&self) -> u16;
}

impl<T: ModeledError + ?Sized> ModeledError for Box<T> {
    fn schema(&self) -> &Schema<'_> {
        (**self).schema()
    }
}

impl<T: HttpModeledError + ?Sized> HttpModeledError for Box<T> {
    fn status_code(&self) -> u16 {
        (**self).status_code()
    }
}
