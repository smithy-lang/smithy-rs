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
///
/// Its supertrait's method is visible through a `SchemaDocument` bound, but only this crate can
/// call it. Use [`DiscriminatedDocumentExt::as_shape`](crate::document::DiscriminatedDocumentExt::as_shape)
/// or [`DiscriminatedDocumentV2Ext::as_shape_v2`](crate::document::DiscriminatedDocumentV2Ext::as_shape_v2)
/// instead:
///
/// ```compile_fail,E0061
/// use aws_smithy_schema::document::SchemaDocument;
/// use aws_smithy_types::DiscriminatedDocument;
///
/// fn read<D: SchemaDocument>(document: &DiscriminatedDocument<D>) {
///     D::with_deserializer(document, |_| ());
/// }
/// ```
pub trait SchemaDocument: DocumentTrait + sealed::Sealed {}

impl SchemaDocument for Document {}
impl SchemaDocument for DocumentV2 {}

/// Calls `read` with a deserializer for `document`'s value and settings.
pub(crate) fn with_deserializer<D: SchemaDocument, R>(
    document: &DiscriminatedDocument<D>,
    read: impl FnOnce(&mut dyn ShapeDeserializer) -> R,
) -> R {
    D::with_deserializer(sealed::Token, document, read)
}

mod sealed {
    use super::*;

    /// The per-document-type behavior of [`SchemaDocument`].
    ///
    /// The trait is public so that it can be a supertrait of `SchemaDocument`. Its module isn't
    /// public, so no other crate can implement it. A `SchemaDocument` bound still makes its
    /// method visible to other crates, so the method takes a [`Token`], which they can't create.
    pub trait Sealed: Sized {
        /// Calls `read` with a deserializer for `document`'s value and settings.
        fn with_deserializer<R>(
            _: Token,
            document: &DiscriminatedDocument<Self>,
            read: impl FnOnce(&mut dyn ShapeDeserializer) -> R,
        ) -> R;
    }

    /// An argument of every [`Sealed`] method, so that only this crate can call them.
    ///
    /// Other crates can't name this type, because its module is private, so they can't create
    /// one.
    #[derive(Debug)]
    pub struct Token;

    impl Sealed for Document {
        fn with_deserializer<R>(
            _: Token,
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
            _: Token,
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
