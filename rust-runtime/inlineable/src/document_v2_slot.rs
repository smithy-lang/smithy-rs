/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Storage for the private `DocumentV2` companions of generated document members.
//!
//! A generated structure keeps each document member's public `Document` field and stores the member's
//! `DocumentV2` value next to it in a [`DocumentV2Slot`]. The builder's `build()` derives the public field from the
//! companion with [`public_value`].

#![allow(dead_code)]

use aws_smithy_types::{Document, DocumentV2};
use std::collections::HashMap;
use std::hash::Hash;

/// A struct's private `DocumentV2` container.
///
/// `PartialEq` always returns `true`, so a struct only compares equal when its other fields do, as it did
/// before it had companions. A deserialized value, which holds both forms, still equals the same value
/// built with the `Document` setters.
///
/// There is no `Debug` impl, so that a struct holding a slot can't derive `Debug` and print the member twice.
#[derive(Clone, Default)]
pub(crate) struct DocumentV2Slot<T>(pub(crate) Option<T>);

impl<T> PartialEq for DocumentV2Slot<T> {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// Converts a companion value to the public field's value: each `DocumentV2` with
/// [`DocumentV2::to_document_lossy`], keeping the surrounding lists, maps, and options.
pub(crate) trait ToDocumentLossy {
    type Document;

    fn to_document_lossy(&self) -> Self::Document;
}

impl ToDocumentLossy for DocumentV2 {
    type Document = Document;

    fn to_document_lossy(&self) -> Document {
        // Inherent methods take precedence, so this calls `DocumentV2`'s own conversion.
        DocumentV2::to_document_lossy(self)
    }
}

impl<T: ToDocumentLossy> ToDocumentLossy for Option<T> {
    type Document = Option<T::Document>;

    fn to_document_lossy(&self) -> Self::Document {
        self.as_ref().map(T::to_document_lossy)
    }
}

impl<T: ToDocumentLossy> ToDocumentLossy for Vec<T> {
    type Document = Vec<T::Document>;

    fn to_document_lossy(&self) -> Self::Document {
        self.iter().map(T::to_document_lossy).collect()
    }
}

impl<K: Clone + Eq + Hash, T: ToDocumentLossy> ToDocumentLossy for HashMap<K, T> {
    type Document = HashMap<K, T::Document>;

    fn to_document_lossy(&self) -> Self::Document {
        self.iter()
            .map(|(key, value)| (key.clone(), value.to_document_lossy()))
            .collect()
    }
}

/// Returns the value that `build()` stores in a document member's public field: the companion's value converted
/// with [`ToDocumentLossy`] when the builder has one, and otherwise the builder's `Document` value.
pub(crate) fn public_value<T: ToDocumentLossy>(
    companion: Option<&T>,
    public: Option<T::Document>,
) -> Option<T::Document> {
    match companion {
        Some(companion) => Some(companion.to_document_lossy()),
        None => public,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob() -> DocumentV2 {
        DocumentV2::Blob(vec![1, 2, 3])
    }

    #[test]
    fn slots_always_compare_equal() {
        assert!(DocumentV2Slot(Some(blob())) == DocumentV2Slot(None));
        assert!(DocumentV2Slot::<DocumentV2>(None) == DocumentV2Slot(None));
        assert!(DocumentV2Slot(Some(blob())) == DocumentV2Slot(Some(DocumentV2::Null)));
    }

    #[test]
    fn default_slot_is_empty() {
        assert_eq!(DocumentV2Slot::<DocumentV2>::default().0, None);
    }

    #[test]
    fn clone_keeps_the_value() {
        let slot = DocumentV2Slot(Some(blob()));
        assert_eq!(slot.clone().0, Some(blob()));
    }

    #[test]
    fn public_value_prefers_the_companion() {
        let public = Some(Document::from("public"));
        assert_eq!(
            public_value(Some(&blob()), public.clone()),
            Some(Document::from("AQID"))
        );
        assert_eq!(public_value::<DocumentV2>(None, public.clone()), public);
        assert_eq!(public_value::<DocumentV2>(None, None), None);
    }

    #[test]
    fn collections_convert_each_document() {
        let list = vec![blob(), DocumentV2::from(true)];
        assert_eq!(
            list.to_document_lossy(),
            vec![Document::from("AQID"), Document::from(true)]
        );

        let sparse = vec![Some(blob()), None];
        assert_eq!(
            sparse.to_document_lossy(),
            vec![Some(Document::from("AQID")), None]
        );

        let map = HashMap::from([("key".to_string(), vec![blob()])]);
        assert_eq!(
            map.to_document_lossy(),
            HashMap::from([("key".to_string(), vec![Document::from("AQID")])])
        );
    }
}
