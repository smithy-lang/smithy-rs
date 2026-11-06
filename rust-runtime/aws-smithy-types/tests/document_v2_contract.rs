/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Downstream contract for [`DocumentV2`].
//!
//! This file is compiled as a separate crate, so it sees `aws_smithy_types` the way a
//! downstream consumer does. It asserts that:
//!
//! 1. `DocumentV2` has the ten Smithy data-model variants, and a downstream `match` compiles
//!    with a wildcard arm. The `compile_fail` doctest on `DocumentV2` asserts that the
//!    wildcard is required, which is what `#[non_exhaustive]` guarantees.
//! 2. `DocumentV2::Object` wraps a `DocumentObject` that keeps insertion order, and the
//!    object's iterator types are nameable from `document::document_object`.
//! 3. `DocumentV2` is exported from the crate root and from `document`, like `Document`.
//! 4. `DiscriminatedDocument<DocumentV2>` wraps a `DocumentV2`, and code bounded by
//!    `DocumentTrait` accepts both wrappers. One `DocumentSettings` implementation works with
//!    both.
//! 5. `From` converts an owned or borrowed `Document` to a `DocumentV2`, and
//!    `to_document_lossy` and `try_to_document` convert back.
//!
//! The `Document` contract is in `document_api_contract.rs` and must not change. The released
//! `DiscriminatedDocument` call forms are in `discriminated_document_contract.rs`.

use aws_smithy_types::document::document_object::{Iter, IterMut, Keys, Values, ValuesMut};
use aws_smithy_types::document::{self, DocumentObject, DocumentTrait};
use aws_smithy_types::{
    BigDecimal, BigInteger, DateTime, DiscriminatedDocument, Document, DocumentError,
    DocumentSettings, DocumentV2, Number,
};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

fn variant_name(document: &DocumentV2) -> &'static str {
    match document {
        DocumentV2::Object(_) => "object",
        DocumentV2::Array(_) => "array",
        DocumentV2::Number(_) => "number",
        DocumentV2::String(_) => "string",
        DocumentV2::Bool(_) => "bool",
        DocumentV2::Null => "null",
        DocumentV2::Blob(_) => "blob",
        DocumentV2::Timestamp(_) => "timestamp",
        DocumentV2::BigInteger(_) => "bigInteger",
        DocumentV2::BigDecimal(_) => "bigDecimal",
        // Required because `DocumentV2` is `#[non_exhaustive]`.
        _ => "unknown",
    }
}

#[test]
fn every_variant_is_constructible_and_matchable() {
    let cases = [
        (DocumentV2::Object(DocumentObject::new()), "object"),
        (DocumentV2::Array(Vec::new()), "array"),
        (DocumentV2::Number(Number::PosInt(1)), "number"),
        (DocumentV2::String("s".to_owned()), "string"),
        (DocumentV2::Bool(true), "bool"),
        (DocumentV2::Null, "null"),
        (DocumentV2::Blob(vec![0xff]), "blob"),
        (DocumentV2::Timestamp(DateTime::from_secs(0)), "timestamp"),
        (
            DocumentV2::BigInteger(BigInteger::from_str("1").unwrap()),
            "bigInteger",
        ),
        (
            DocumentV2::BigDecimal(BigDecimal::from_str("1.5").unwrap()),
            "bigDecimal",
        ),
    ];
    for (document, expected) in cases {
        assert_eq!(variant_name(&document), expected);
        assert_eq!(document.clone(), document);
        assert!(!format!("{document:?}").is_empty());
    }
}

#[test]
fn object_preserves_insertion_order() {
    let mut object = DocumentObject::new();
    for key in ["z", "a", "m"] {
        object.insert(key.to_owned(), DocumentV2::Null);
    }
    let mut document = DocumentV2::Object(object);

    let object = document.as_object().unwrap();
    let keys: Keys<'_> = object.keys();
    assert_eq!(
        keys.map(String::as_str).collect::<Vec<_>>(),
        ["z", "a", "m"]
    );
    let entries: Iter<'_> = object.iter();
    assert_eq!(entries.len(), 3);
    let mut values: Values<'_> = object.values();
    assert!(values.all(DocumentV2::is_null));

    let object = document.as_object_mut().unwrap();
    let entries: IterMut<'_> = object.iter_mut();
    assert_eq!(entries.len(), 3);
    let values: ValuesMut<'_> = object.values_mut();
    for value in values {
        *value = DocumentV2::Bool(true);
    }
    let owned: Vec<(String, DocumentV2)> = object.clone().into_iter().collect();
    assert_eq!(owned[0], ("z".to_owned(), DocumentV2::Bool(true)));
}

#[test]
fn document_v2_is_exported_from_the_root_and_the_document_module() {
    // Compiles only if both paths name the same type.
    fn document_v2(value: document::DocumentV2) -> DocumentV2 {
        value
    }
    assert_eq!(document_v2(DocumentV2::Null), DocumentV2::Null);
}

#[test]
fn the_wrapper_holds_a_document_v2() {
    let wrapper: DiscriminatedDocument<DocumentV2> =
        DiscriminatedDocument::new_v2(DocumentV2::Blob(b"raw".to_vec()));
    let document: &DocumentV2 = wrapper.document();
    assert_eq!(document.as_blob(), Some(b"raw".as_slice()));
    assert_eq!(wrapper.as_blob().unwrap().as_ref(), b"raw");

    let from: DiscriminatedDocument<DocumentV2> = DocumentV2::Null.into();
    let owned: DocumentV2 = from.into_document();
    assert_eq!(owned, DocumentV2::Null);
}

#[test]
fn document_trait_bounds_accept_both_wrappers() {
    fn discriminator<D: DocumentTrait>(document: &DiscriminatedDocument<D>) -> Option<&str> {
        document.discriminator()
    }
    let v1 = DiscriminatedDocument::new(Document::Null)
        .try_with_discriminator("com.example#Shape")
        .unwrap();
    let v2 = DiscriminatedDocument::new_v2(DocumentV2::Null)
        .try_with_discriminator("com.example#Shape")
        .unwrap();
    assert_eq!(discriminator(&v1), discriminator(&v2));
}

#[test]
fn one_settings_implementation_serves_both_wrappers() {
    #[derive(Debug)]
    struct Settings;
    impl DocumentSettings for Settings {
        fn protocol_id(&self) -> &str {
            "com.example#Protocol"
        }
    }
    let settings: Arc<dyn DocumentSettings> = Arc::new(Settings);

    let v1 = DiscriminatedDocument::new(Document::Null).with_settings(settings.clone());
    let v2 = DiscriminatedDocument::new_v2(DocumentV2::Null).with_settings(settings);
    assert_eq!(
        v1.settings().unwrap().protocol_id(),
        v2.settings().unwrap().protocol_id()
    );
}

#[test]
fn discriminators_must_be_absolute() {
    let document = DiscriminatedDocument::new_v2(DocumentV2::Null)
        .try_with_discriminator("com.example#Shape")
        .unwrap();
    assert_eq!(document.discriminator(), Some("com.example#Shape"));

    assert!(DiscriminatedDocument::new_v2(DocumentV2::Null)
        .try_with_discriminator("Shape")
        .is_err());
}

#[test]
fn documents_upgrade_with_from_and_into() {
    let document = Document::Object(HashMap::from([(
        "list".to_owned(),
        Document::Array(vec![Document::Bool(true), Document::Null]),
    )]));
    let expected = DocumentV2::Object(DocumentObject::from([(
        "list".to_owned(),
        DocumentV2::Array(vec![DocumentV2::Bool(true), DocumentV2::Null]),
    )]));

    assert_eq!(DocumentV2::from(&document), expected);
    let borrowed: DocumentV2 = (&document).into();
    assert_eq!(borrowed, expected);
    let owned: DocumentV2 = document.clone().into();
    assert_eq!(owned, expected);
    assert_eq!(DocumentV2::from(document), expected);
}

#[test]
fn documents_downgrade_with_named_methods() {
    let document = DocumentV2::Array(vec![
        DocumentV2::String("s".to_owned()),
        DocumentV2::Blob(b"hi".to_vec()),
    ]);

    let lossy: Document = document.to_document_lossy();
    assert_eq!(
        lossy,
        Document::Array(vec![
            Document::String("s".to_owned()),
            Document::String("aGk=".to_owned()),
        ])
    );

    let result: Result<Document, DocumentError> = document.try_to_document();
    match result {
        Err(DocumentError::TypeMismatch { message, .. }) => {
            assert_eq!(message, "Document can't represent the blob at $[1]")
        }
        other => panic!("expected a type mismatch, got {other:?}"),
    }

    let representable = DocumentV2::from(Document::Bool(true));
    assert_eq!(
        representable.try_to_document().unwrap(),
        Document::Bool(true)
    );
}
