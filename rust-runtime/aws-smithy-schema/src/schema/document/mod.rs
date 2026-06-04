/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! `Document` — a protocol-agnostic representation of any value in the
//! Smithy data model — and the [`ShapeSerializer`] / [`ShapeDeserializer`]
//! adapters that bridge `Document` with arbitrary Smithy shapes.
//!
//! The submodules are organized as:
//!
//! - [`data`] — the [`Document`] data type, [`DocumentInner`] enum,
//!   [`DocumentSettings`] trait, scalar accessors, and conversions
//!   to/from `aws_smithy_types::Document`.
//! - [`serializer`] — the [`DocumentShapeSerializer`], a
//!   [`ShapeSerializer`](crate::serde::ShapeSerializer) implementation
//!   that builds a `Document` tree from a typed shape.
//! - [`deserializer`] — the [`DocumentShapeDeserializer`], a
//!   [`ShapeDeserializer`](crate::serde::ShapeDeserializer) implementation
//!   that walks a `Document` tree and dispatches to consumer callbacks
//!   defined by generated `deserialize` methods.
//!
//! See the SEP "Document Type and Type Registries" for the full
//! specification.

mod data;
mod deserializer;
#[cfg(test)]
mod round_trips;
mod serializer;

pub use data::{Document, DocumentInner, DocumentSettings};
pub use deserializer::DocumentShapeDeserializer;
pub use serializer::DocumentShapeSerializer;
