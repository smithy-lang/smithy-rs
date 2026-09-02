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
//! - [`HttpServerError`]: the erased server-protocol error seam. Modeled
//!   errors automatically implement it; non-modeled framework errors implement
//!   it directly.
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

use std::any::Any;

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

/// HTTP extension of [`ModeledError`].
///
/// The `Debug + Display + Send + Sync` supertraits exist because boxed
/// values of this trait travel through
/// `RequestRejection::ConstraintViolation`: `RequestRejection` derives
/// `Debug`, `Upgrade` logs rejections via `Display`, rejections cross await
/// points, and the rejection's `Serialization` fallback converts into
/// `crate::Error` (`Send + Sync`). `Display` is free for generated `@error`
/// shapes — they implement `std::error::Error` — and generated error shapes
/// are plain data, so `Send + Sync` hold structurally.
pub trait HttpModeledError: ModeledError + HttpServerError {
    /// The HTTP status code for this error.
    ///
    /// Generated implementations bake a literal resolved at codegen time:
    /// the `@httpError` code if present, else the `@error` fault default
    /// (client = 400, server = 500).
    fn status_code(&self) -> u16;
}

/// Something an HTTP server protocol knows how to put on the wire.
///
/// Modeled Smithy errors are exposed through [`as_modeled_error`]. Framework
/// errors that are not Smithy shapes implement this trait directly and may be
/// downcast by protocols for backward-compatible wire behavior.
///
/// [`as_modeled_error`]: HttpServerError::as_modeled_error
pub trait HttpServerError: std::error::Error + Send + Sync + 'static {
    /// HTTP status code for this server error.
    fn status_code(&self) -> u16;

    /// Returns this value as a Smithy modeled error when applicable.
    fn as_modeled_error(&self) -> Option<&dyn HttpModeledError> {
        None
    }

    /// Type-erased access for protocol-specific framework errors.
    fn as_any(&self) -> &dyn Any;
}

/// Type-erased server protocol error.
///
/// Protocol implementations convert their private request rejection types into
/// this before crossing the static-to-dynamic protocol boundary.
pub type ServerError = Box<dyn HttpServerError>;

impl<T> HttpServerError for T
where
    T: HttpModeledError,
{
    fn status_code(&self) -> u16 {
        HttpModeledError::status_code(self)
    }

    fn as_modeled_error(&self) -> Option<&dyn HttpModeledError> {
        Some(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: ModeledError + ?Sized> ModeledError for Box<T> {
    fn schema(&self) -> &Schema<'_> {
        (**self).schema()
    }
}

impl<T: HttpModeledError> HttpModeledError for Box<T> {
    fn status_code(&self) -> u16 {
        HttpModeledError::status_code(&**self)
    }
}
