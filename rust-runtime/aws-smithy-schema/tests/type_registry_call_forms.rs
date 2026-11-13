/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Call forms of `TypeRegistry::deserialize_document`, from an external crate.
//!
//! Released versions of `aws-smithy-schema` declare the method as taking
//! `&DiscriminatedDocument`, which means `&DiscriminatedDocument<Document>`. It now takes
//! `&DiscriminatedDocument<impl SchemaDocument>`. The argument-position `impl Trait` keeps the
//! method free of named generic parameters, so it accepts no turbofish, as before. The first
//! test uses the method the ways released code can, including forms that rely on the parameter
//! type for inference or coercion. Each must still compile, with `Document` inferred.

use std::sync::Arc;

use aws_smithy_schema::document::SchemaDocument;
use aws_smithy_schema::prelude;
use aws_smithy_schema::registry::TypeRegistry;
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::{DiscriminatedDocument, Document, DocumentV2};

type Output = Result<TypeErasedBox, SerdeError>;

fn deserialize_string(deser: &mut dyn ShapeDeserializer) -> Output {
    Ok(TypeErasedBox::new(deser.read_string(&prelude::STRING)?))
}

fn registry() -> TypeRegistry {
    TypeRegistry::builder()
        .insert_shape(&prelude::STRING, deserialize_string)
        .build()
}

fn hello() -> DiscriminatedDocument {
    DiscriminatedDocument::new(Document::String("hello".to_string()))
        .with_discriminator("smithy.api#String")
}

fn read(output: Output) -> String {
    *output.unwrap().downcast::<String>().unwrap()
}

/// Accepts a function with the released signature.
fn call_with(f: impl Fn(&TypeRegistry, &DiscriminatedDocument) -> Output) -> Output {
    f(&registry(), &hello())
}

#[test]
#[allow(clippy::needless_borrow, clippy::redundant_closure)]
fn released_call_forms_compile_and_read_a_document() {
    let registry = registry();
    let document = hello();

    assert_eq!(read(registry.deserialize_document(&document)), "hello");
    assert_eq!(
        read(TypeRegistry::deserialize_document(&registry, &document)),
        "hello"
    );

    // Deref coercions to `&DiscriminatedDocument`.
    let reference = &document;
    assert_eq!(read(registry.deserialize_document(&reference)), "hello");
    let boxed = Box::new(hello());
    assert_eq!(read(registry.deserialize_document(&boxed)), "hello");
    let shared = Arc::new(hello());
    assert_eq!(read(registry.deserialize_document(&shared)), "hello");

    // Arguments whose type comes from the parameter.
    assert!(registry.deserialize_document(&Default::default()).is_err());
    assert!(registry
        .deserialize_document(&Document::Null.into())
        .is_err());

    // The method as a function value.
    let f: fn(&TypeRegistry, &DiscriminatedDocument) -> Output = TypeRegistry::deserialize_document;
    assert_eq!(read(f(&registry, &document)), "hello");
    assert_eq!(read(call_with(TypeRegistry::deserialize_document)), "hello");
    assert_eq!(
        read(call_with(
            |registry, document| registry.deserialize_document(document)
        )),
        "hello"
    );

    let documents = [hello(), hello()];
    let values: Vec<String> = documents
        .iter()
        .map(|document| read(registry.deserialize_document(document)))
        .collect();
    assert_eq!(values, ["hello", "hello"]);
}

/// External code can accept either document type through `SchemaDocument`.
fn deserialize_either<D: SchemaDocument>(document: &DiscriminatedDocument<D>) -> String {
    read(registry().deserialize_document(document))
}

#[test]
fn either_document_type_resolves_the_same_entry() {
    let v2 = DiscriminatedDocument::new_v2(DocumentV2::String("hello".to_string()))
        .try_with_discriminator("smithy.api#String")
        .unwrap();
    assert_eq!(read(registry().deserialize_document(&v2)), "hello");
    assert_eq!(deserialize_either(&v2), "hello");
    assert_eq!(deserialize_either(&hello()), "hello");
}
