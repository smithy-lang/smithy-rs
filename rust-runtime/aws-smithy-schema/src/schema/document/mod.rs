/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Schema-aware adapters for the [`Document`](aws_smithy_types::Document) data model.
//!
//! The [`Document`](aws_smithy_types::Document), [`DiscriminatedDocument`](aws_smithy_types::DiscriminatedDocument), and [`DocumentSettings`](aws_smithy_types::DocumentSettings)
//! types live in `aws-smithy-types` (the dependency direction is
//! `aws-smithy-schema → aws-smithy-types`); import them from there. This
//! module supplies the schema-crate-only adapters that connect `Document`
//! with the [`ShapeSerializer`](crate::serde::ShapeSerializer) /
//! [`ShapeDeserializer`](crate::serde::ShapeDeserializer) machinery:
//!
//! - [`serializer`] — the [`DocumentShapeSerializer`], a
//!   [`ShapeSerializer`](crate::serde::ShapeSerializer) implementation
//!   that builds a [`Document`](aws_smithy_types::Document) tree from a
//!   typed Smithy shape.
//! - [`deserializer`] — the [`DocumentShapeDeserializer`], a
//!   [`ShapeDeserializer`](crate::serde::ShapeDeserializer) implementation
//!   that walks a [`Document`](aws_smithy_types::Document) tree and
//!   dispatches to consumer callbacks defined by generated `deserialize`
//!   methods.
//! - [`discriminated_ext`] — [`DiscriminatedDocumentExt`], an extension
//!   trait that bolts `Document.from_struct` / `Document.asShape` /
//!   `Document.shape_type` onto
//!   [`DiscriminatedDocument`](aws_smithy_types::DiscriminatedDocument)
//!   (which itself lives in `aws-smithy-types` and so cannot reference
//!   [`Schema`](crate::Schema) inherently).
//!
//! [`DocumentV2`](aws_smithy_types::DocumentV2) has the same three adapters:
//! [`DocumentV2ShapeSerializer`], [`DocumentV2ShapeDeserializer`], and
//! [`DiscriminatedDocumentV2Ext`]. They keep blobs, timestamps, and arbitrary-precision
//! numbers as native values, and keep the order of structure members and map entries.
//! [`SchemaDocument`] is implemented by both document types, for code such as
//! [`TypeRegistry::deserialize_document`](crate::registry::TypeRegistry::deserialize_document)
//! that accepts either wrapper.
//!
//! See the SEP "Document Type and Type Registries" for the full
//! specification.

mod deserializer;
mod deserializer_v2;
mod discriminated_ext;
mod discriminated_ext_v2;
#[cfg(test)]
mod round_trips;
mod schema_document;
mod serializer;
mod serializer_v2;

pub use deserializer::DocumentShapeDeserializer;
pub use deserializer_v2::DocumentV2ShapeDeserializer;
pub use discriminated_ext::DiscriminatedDocumentExt;
pub use discriminated_ext_v2::DiscriminatedDocumentV2Ext;
pub(crate) use schema_document::with_deserializer;
pub use schema_document::SchemaDocument;
pub use serializer::DocumentShapeSerializer;
pub use serializer_v2::DocumentV2ShapeSerializer;
