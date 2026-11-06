/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Downstream source-compatibility contract for [`DiscriminatedDocument`].
//!
//! This file is compiled as a separate crate, so it sees `aws_smithy_types` the way a
//! downstream consumer does. Released crates, including `aws-smithy-schema` 1.0.0 through
//! 1.1.2 and `aws-smithy-json` 0.65.0 and 0.65.1, were compiled against a
//! `DiscriminatedDocument` with no type parameter, as published in `aws-smithy-types` 1.9.1.
//! Cargo can pair those crates with any later 1.x release, so every call form here must keep
//! compiling.
//!
//! `DiscriminatedDocument` is now generic over its document type, with `Document` as the
//! default. Rust doesn't use a type parameter's default when it infers a type, so most of these
//! cases guard against a specific mistake:
//!
//! - Making `new` generic breaks `new("text".into())` and `new(Default::default())`.
//! - Implementing `Default` for a second document type breaks `DiscriminatedDocument::default()`.
//! - Defining an inherent method name for both document types breaks path calls such as
//!   `DiscriminatedDocument::as_blob(&document)`.
//!
//! The compile-time half of the contract is enforced by this file compiling at all, which
//! every `cargo test` run does.

use aws_smithy_types::{DateTime, DiscriminatedDocument, Document, DocumentSettings, Number};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
struct Settings;

impl DocumentSettings for Settings {
    fn protocol_id(&self) -> &str {
        "com.example#Protocol"
    }
}

#[test]
fn constructors_infer_the_v1_wrapper() {
    let from_value = DiscriminatedDocument::new(Document::Null);
    let from_into = DiscriminatedDocument::new("text".into());
    let from_default_argument = DiscriminatedDocument::new(Default::default());
    let from_hash_map = DiscriminatedDocument::new(HashMap::new().into());
    let default = DiscriminatedDocument::default();
    let from = DiscriminatedDocument::from(Document::Bool(true));
    let into: DiscriminatedDocument = Document::Bool(false).into();
    let mapped: Vec<_> = vec![Document::Null]
        .into_iter()
        .map(DiscriminatedDocument::new)
        .collect();
    let mapped_from: Vec<_> = vec![Document::Null]
        .into_iter()
        .map(DiscriminatedDocument::from)
        .collect();

    assert_eq!(from_value.discriminator(), None);
    assert_eq!(from_into.document().as_string(), Some("text"));
    assert!(from_default_argument.document().is_null());
    assert!(from_hash_map.document().is_object());
    assert!(default.settings().is_none());
    assert_eq!(from.document().as_bool(), Some(true));
    assert_eq!(into.document().as_bool(), Some(false));
    assert_eq!(mapped, mapped_from);
}

#[test]
fn builder_methods_and_accessors_keep_their_types() {
    let document = DiscriminatedDocument::new(Document::Number(Number::PosInt(1)))
        .with_discriminator("com.example#Shape")
        .with_settings(Arc::new(Settings));

    let discriminator: Option<&str> = document.discriminator();
    let settings: Option<&Arc<dyn DocumentSettings>> = document.settings();
    let inner: &Document = document.document();
    assert_eq!(discriminator, Some("com.example#Shape"));
    assert_eq!(settings.unwrap().protocol_id(), "com.example#Protocol");
    assert_eq!(inner, &Document::Number(Number::PosInt(1)));

    let owned: Document = document.clone().into_document();
    assert_eq!(owned, Document::Number(Number::PosInt(1)));
    assert_eq!(document.clone(), document);
    assert!(!format!("{document:?}").is_empty());
}

#[test]
fn path_calls_and_function_items_resolve() {
    let document = DiscriminatedDocument::new(Document::String("YWJjZA==".to_owned()));

    let blob: Cow<'_, [u8]> = DiscriminatedDocument::as_blob(&document).unwrap();
    assert_eq!(blob.as_ref(), b"abcd");
    let timestamp: Result<DateTime, _> = DiscriminatedDocument::as_timestamp(&document);
    assert!(timestamp.is_err());
    assert!(DiscriminatedDocument::document(&document).is_string());
    assert_eq!(DiscriminatedDocument::discriminator(&document), None);
    assert!(DiscriminatedDocument::settings(&document).is_none());

    let with_discriminator = DiscriminatedDocument::with_discriminator(document, "com.example#A");
    let with_settings =
        DiscriminatedDocument::with_settings(with_discriminator, Arc::new(Settings));
    let inner: Document = DiscriminatedDocument::into_document(with_settings);
    assert!(inner.is_string());

    let discriminator: fn(&DiscriminatedDocument) -> Option<&str> =
        DiscriminatedDocument::discriminator;
    let document: fn(&DiscriminatedDocument) -> &Document = DiscriminatedDocument::document;
    let wrapper = DiscriminatedDocument::new(Document::Null);
    assert_eq!(discriminator(&wrapper), None);
    assert!(document(&wrapper).is_null());
}

#[test]
fn type_position_uses_mean_the_v1_wrapper() {
    // A downstream trait implemented for the unparameterized name, as `aws-smithy-schema`
    // does for `DiscriminatedDocumentExt`.
    trait Describe {
        fn describe(&self) -> &'static str;
    }
    impl Describe for DiscriminatedDocument {
        fn describe(&self) -> &'static str {
            "v1"
        }
    }

    #[derive(Default)]
    struct Holder {
        document: DiscriminatedDocument,
    }

    fn inner(document: &DiscriminatedDocument) -> &Document {
        document.document()
    }

    fn missing() -> Option<DiscriminatedDocument> {
        None
    }

    let holder = Holder::default();
    assert_eq!(holder.document.describe(), "v1");
    assert!(inner(&holder.document).is_null());
    assert!(missing().unwrap_or_default().document().is_null());
}

#[test]
fn the_wrapper_is_send_sync_and_static() {
    fn assert_bounds<T: Send + Sync + 'static>() {}
    assert_bounds::<DiscriminatedDocument>();
}
