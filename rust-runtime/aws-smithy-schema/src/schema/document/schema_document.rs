/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`SchemaDocument`], implemented by both document types.

use aws_smithy_types::document::DocumentTrait;
use aws_smithy_types::{DiscriminatedDocument, Document, DocumentV2};

use super::{DocumentShapeDeserializer, DocumentV2ShapeDeserializer};
use crate::serde::ShapeDeserializer;

/// A document type that schema-driven code can read shapes from: [`Document`] or
/// [`DocumentV2`].
///
/// Use it as a bound to accept either [`DiscriminatedDocument`] wrapper, as
/// [`TypeRegistry::deserialize_document`](crate::registry::TypeRegistry::deserialize_document)
/// does. A `Document` is read with a [`DocumentShapeDeserializer`] and a `DocumentV2` with a
/// [`DocumentV2ShapeDeserializer`], each with the wrapper's settings.
///
/// The trait is sealed, so only this crate can implement it.
pub trait SchemaDocument: DocumentTrait + sealed::Sealed {}

impl SchemaDocument for Document {}
impl SchemaDocument for DocumentV2 {}

/// Calls `read` with a deserializer for `document`'s value and settings.
pub(crate) fn with_deserializer<D: SchemaDocument, R>(
    document: &DiscriminatedDocument<D>,
    read: impl FnOnce(&mut dyn ShapeDeserializer) -> R,
) -> R {
    D::with_deserializer(document, read)
}

mod sealed {
    use super::*;

    /// The per-document-type behavior of [`SchemaDocument`].
    ///
    /// The trait is public so that it can be a supertrait of `SchemaDocument`. Its module isn't
    /// public, so no other crate can implement it.
    pub trait Sealed: Sized {
        /// Calls `read` with a deserializer for `document`'s value and settings.
        fn with_deserializer<R>(
            document: &DiscriminatedDocument<Self>,
            read: impl FnOnce(&mut dyn ShapeDeserializer) -> R,
        ) -> R;
    }

    impl Sealed for Document {
        fn with_deserializer<R>(
            document: &DiscriminatedDocument<Self>,
            read: impl FnOnce(&mut dyn ShapeDeserializer) -> R,
        ) -> R {
            read(&mut DocumentShapeDeserializer::new_with_settings(
                document.document(),
                document.settings().cloned(),
            ))
        }
    }

    impl Sealed for DocumentV2 {
        fn with_deserializer<R>(
            document: &DiscriminatedDocument<Self>,
            read: impl FnOnce(&mut dyn ShapeDeserializer) -> R,
        ) -> R {
            read(&mut DocumentV2ShapeDeserializer::new_with_settings(
                document.document(),
                document.settings().cloned(),
            ))
        }
    }
}
