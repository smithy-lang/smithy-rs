/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Storage for the private `DocumentV2` companions of generated document members.
//!
//! A generated structure keeps each document member's public `Document` field and stores the member's
//! `DocumentV2` value next to it in a [`DocumentV2Slot`]. The builder's `build()` derives the public field from the
//! companion with [`public_value`].
//!
//! The public field can be changed after the structure is built, and the slot can't see that. So a companion is
//! only used while it is current: while the public field still holds the companion's [`ToDocumentLossy`]
//! conversion, as [`current_companion`] checks. After the public field changes, the structure's getter returns
//! `None`, serializers write the public field, and `into_builder()` leaves the companion out.

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
        self.as_slice().to_document_lossy()
    }
}

/// For a list companion's getter, which returns a slice.
impl<T: ToDocumentLossy> ToDocumentLossy for [T] {
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

/// Returns `companion` while it is current: while `public`, the structure's public field, still holds the
/// companion's [`ToDocumentLossy`] conversion. Otherwise the public field was changed after the structure was
/// built or read, and this returns `None`, so that the change takes effect.
///
/// A public value equal to the conversion can't be told apart from an unchanged one, so it keeps the companion,
/// which is that value's exact form. A companion whose conversion doesn't equal itself, such as one holding a NaN
/// float, is never current. The public field then holds the same value.
pub(crate) fn current_companion<'a, T>(
    companion: Option<&'a T>,
    public: Option<&T::Document>,
) -> Option<&'a T>
where
    T: ToDocumentLossy,
    T::Document: PartialEq,
{
    let companion = companion?;
    (public? == &companion.to_document_lossy()).then_some(companion)
}

/// [`current_companion`] for an owned companion, as `into_builder()` moves it into the builder.
pub(crate) fn into_current_companion<T>(
    companion: Option<T>,
    public: Option<&T::Document>,
) -> Option<T>
where
    T: ToDocumentLossy,
    T::Document: PartialEq,
{
    let is_current = current_companion(companion.as_ref(), public).is_some();
    companion.filter(|_| is_current)
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
        assert_eq!(
            sparse.as_slice().to_document_lossy(),
            vec![Some(Document::from("AQID")), None]
        );

        let map = HashMap::from([("key".to_string(), vec![blob()])]);
        assert_eq!(
            map.to_document_lossy(),
            HashMap::from([("key".to_string(), vec![Document::from("AQID")])])
        );
    }

    #[test]
    fn a_companion_is_current_while_the_public_field_holds_its_conversion() {
        let companion = blob();
        let unchanged = Document::from("AQID");
        assert_eq!(
            current_companion(Some(&companion), Some(&unchanged)),
            Some(&companion)
        );
        assert_eq!(
            into_current_companion(Some(blob()), Some(&unchanged)),
            Some(blob())
        );

        let changed = Document::from("changed");
        assert_eq!(current_companion(Some(&companion), Some(&changed)), None);
        assert_eq!(into_current_companion(Some(blob()), Some(&changed)), None);

        // Clearing the public field makes the companion stale too.
        assert_eq!(current_companion(Some(&companion), None), None);
        assert_eq!(
            current_companion::<DocumentV2>(None, Some(&unchanged)),
            None
        );
    }

    #[test]
    fn a_change_inside_a_collection_makes_the_companion_stale() {
        let list = vec![blob(), DocumentV2::from(true)];
        let mut public = list.to_document_lossy();
        assert_eq!(current_companion(Some(&list), Some(&public)), Some(&list));
        public[1] = Document::from(false);
        assert_eq!(current_companion(Some(&list), Some(&public)), None);

        let sparse = vec![Some(blob()), None];
        let mut public = sparse.to_document_lossy();
        assert_eq!(
            current_companion(Some(&sparse), Some(&public)),
            Some(&sparse)
        );
        public[1] = Some(Document::Null);
        assert_eq!(current_companion(Some(&sparse), Some(&public)), None);

        let map = HashMap::from([("key".to_string(), blob())]);
        let mut public = map.to_document_lossy();
        assert_eq!(current_companion(Some(&map), Some(&public)), Some(&map));
        public.insert("other".to_string(), Document::Null);
        assert_eq!(current_companion(Some(&map), Some(&public)), None);
    }

    #[test]
    fn a_nan_companion_is_never_current() {
        let nan = DocumentV2::Number(aws_smithy_types::Number::Float(f64::NAN));
        let public = nan.to_document_lossy();
        assert_eq!(current_companion(Some(&nan), Some(&public)), None);
    }
}
