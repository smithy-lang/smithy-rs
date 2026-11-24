/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Records the `registry` API that generated code and downstream crates depend on.
//!
//! Generated clients and servers emit `LazyLock<TypeRegistry>` statics, return
//! `&'static TypeRegistry` from their accessors, and call [`reify_error`] without being
//! regenerated when the runtime crates are upgraded. Downstream crates may also name these
//! types in impl headers and rely on the `'static` lifetimes of the schemas they return.
//!
//! This file uses the API the way that generated code does, so a change that fails to compile
//! here is a source-breaking change. Examples include adding a lifetime or type parameter to
//! a registry type, making an untyped constructor ambiguous, shortening a returned schema
//! lifetime, or dropping an auto trait.

#![deny(elided_lifetimes_in_paths)]

use std::fmt::Debug;
use std::sync::LazyLock;

use aws_smithy_schema::document::DocumentShapeDeserializer;
use aws_smithy_schema::prelude;
use aws_smithy_schema::registry::{
    reify_error, ComposedRegistry, DeserializeFn, ErrorDeserializeFn, Iter, RegistryEntry,
    TypeRegistry, TypeRegistryBuilder,
};
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::{shape_id, Schema, ShapeId};
use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::{DiscriminatedDocument, Document};

type BoxError = Box<dyn std::error::Error + Send + Sync>;

// -- Deserializers with the generated function-pointer signatures -------------------------------

fn deserialize_string(d: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
    Ok(TypeErasedBox::new(d.read_string(&prelude::STRING)?))
}

fn deserialize_string_error(d: &mut dyn ShapeDeserializer) -> Result<BoxError, SerdeError> {
    Ok(d.read_string(&prelude::STRING)?.into())
}

const DESERIALIZE: DeserializeFn = deserialize_string;
const ERROR_DESERIALIZE: ErrorDeserializeFn = deserialize_string_error;

// -- Generated-code shapes ---------------------------------------------------------------------

// `RegistryEntry` constructors are `const` and accept static schemas.
static ENTRY: RegistryEntry = RegistryEntry::new(&prelude::STRING, DESERIALIZE);
static ERROR_ENTRY: RegistryEntry =
    RegistryEntry::new_error(&prelude::STRING, DESERIALIZE, ERROR_DESERIALIZE);

// The generated package registry and its accessor.
static REGISTRY: LazyLock<TypeRegistry> = LazyLock::new(|| {
    TypeRegistry::builder()
        .insert_shape(&prelude::STRING, deserialize_string)
        .build()
});

fn registry() -> &'static TypeRegistry {
    &REGISTRY
}

// The generated service-wide and per-operation error registries.
static SERVICE_ERRORS: LazyLock<TypeRegistry> = LazyLock::new(|| {
    TypeRegistry::builder()
        .insert_error_shape(
            &prelude::STRING,
            deserialize_string,
            deserialize_string_error,
        )
        .build()
});

static OPERATION_ERRORS: LazyLock<TypeRegistry> = LazyLock::new(TypeRegistry::new);

fn error_registry() -> &'static TypeRegistry {
    &SERVICE_ERRORS
}

// -- Impl headers without lifetime arguments ---------------------------------------------------

trait LocalTrait {
    fn local_name(&self) -> &'static str;
}

impl LocalTrait for TypeRegistry {
    fn local_name(&self) -> &'static str {
        "TypeRegistry"
    }
}

impl LocalTrait for TypeRegistryBuilder {
    fn local_name(&self) -> &'static str {
        "TypeRegistryBuilder"
    }
}

impl LocalTrait for RegistryEntry {
    fn local_name(&self) -> &'static str {
        "RegistryEntry"
    }
}

impl LocalTrait for Iter<'_> {
    fn local_name(&self) -> &'static str {
        "Iter"
    }
}

impl LocalTrait for ComposedRegistry<'_> {
    fn local_name(&self) -> &'static str {
        "ComposedRegistry"
    }
}

// -- Auto traits and derives -------------------------------------------------------------------

const _: () = {
    const fn shared<T: Send + Sync + Unpin + Debug>() {}
    const fn copy<T: Copy + Clone>() {}
    const fn default<T: Default>() {}

    shared::<TypeRegistry>();
    shared::<TypeRegistryBuilder>();
    shared::<RegistryEntry>();
    shared::<Iter<'static>>();
    shared::<ComposedRegistry<'static>>();

    copy::<RegistryEntry>();
    copy::<ComposedRegistry<'static>>();

    default::<TypeRegistry>();
    default::<TypeRegistryBuilder>();
};

// -- Signatures --------------------------------------------------------------------------------

/// Pins the exact signature of every public function and method by coercing it to a
/// function pointer type.
#[allow(clippy::type_complexity)]
fn pin_signatures() {
    let _: fn(&'static Schema<'static>, DeserializeFn) -> RegistryEntry = RegistryEntry::new;
    let _: fn(&'static Schema<'static>, DeserializeFn, ErrorDeserializeFn) -> RegistryEntry =
        RegistryEntry::new_error;
    let _: fn(&RegistryEntry) -> &'static Schema<'static> = RegistryEntry::schema;
    let _: fn(&RegistryEntry) -> DeserializeFn = RegistryEntry::deserialize_fn;
    let _: fn(&RegistryEntry) -> Option<ErrorDeserializeFn> = RegistryEntry::error_deserialize_fn;

    let _: fn() -> TypeRegistry = TypeRegistry::new;
    let _: fn() -> TypeRegistryBuilder = TypeRegistry::builder;
    let _: for<'r, 'i> fn(&'r TypeRegistry, &'i ShapeId<'_>) -> Option<&'static Schema<'static>> =
        TypeRegistry::schema_for;
    let _: for<'r, 'f> fn(&'r TypeRegistry, &'f str) -> Option<&'static Schema<'static>> =
        TypeRegistry::schema_for_fqn;
    let _: for<'r> fn(&'r TypeRegistry, &ShapeId<'_>) -> Option<&'r RegistryEntry> =
        TypeRegistry::entry_for;
    let _: for<'r> fn(&'r TypeRegistry, &str) -> Option<&'r RegistryEntry> =
        TypeRegistry::entry_for_fqn;
    let _: for<'r> fn(&'r TypeRegistry, &str) -> Option<&'r RegistryEntry> =
        TypeRegistry::entry_for_error_code;
    let _: fn(&TypeRegistry, &DiscriminatedDocument) -> Result<TypeErasedBox, SerdeError> =
        TypeRegistry::deserialize_document;
    let _: fn(TypeRegistry, TypeRegistry) -> TypeRegistry = TypeRegistry::compose;
    let _: for<'a> fn(&'a TypeRegistry, &'a TypeRegistry) -> ComposedRegistry<'a> =
        TypeRegistry::or;
    let _: for<'r> fn(&'r TypeRegistry) -> Iter<'r> = TypeRegistry::iter;
    let _: fn(&TypeRegistry) -> usize = TypeRegistry::len;
    let _: fn(&TypeRegistry) -> bool = TypeRegistry::is_empty;

    // The impl lifetime of `ComposedRegistry` is early-bound, so it is fixed here.
    let _: for<'c> fn(&'c ComposedRegistry<'static>, &ShapeId<'_>) -> Option<&'c RegistryEntry> =
        ComposedRegistry::entry_for;
    let _: for<'c> fn(&'c ComposedRegistry<'static>, &str) -> Option<&'c RegistryEntry> =
        ComposedRegistry::entry_for_fqn;
    let _: for<'c> fn(&'c ComposedRegistry<'static>, &str) -> Option<&'c RegistryEntry> =
        ComposedRegistry::entry_for_error_code;

    let _: fn(TypeRegistryBuilder, ShapeId<'static>, RegistryEntry) -> TypeRegistryBuilder =
        TypeRegistryBuilder::insert;
    let _: fn(TypeRegistryBuilder, &'static Schema<'static>, DeserializeFn) -> TypeRegistryBuilder =
        TypeRegistryBuilder::insert_shape;
    let _: fn(
        TypeRegistryBuilder,
        &'static Schema<'static>,
        DeserializeFn,
        ErrorDeserializeFn,
    ) -> TypeRegistryBuilder = TypeRegistryBuilder::insert_error_shape;
    let _: fn(TypeRegistryBuilder) -> TypeRegistry = TypeRegistryBuilder::build;

    let _: fn(ComposedRegistry<'_>, &str, &mut dyn ShapeDeserializer) -> Option<BoxError> =
        reify_error;
}

// -- Static return lifetimes -------------------------------------------------------------------

/// Schemas returned by lookups outlive the registry they came from.
fn schema_outlives_registry() -> &'static Schema<'static> {
    let registry = TypeRegistry::builder()
        .insert_shape(&prelude::STRING, deserialize_string)
        .build();
    let by_id = registry
        .schema_for(prelude::STRING.shape_id())
        .expect("registered");
    let by_fqn = registry
        .schema_for_fqn("smithy.api#String")
        .expect("registered");
    assert_eq!(by_id.shape_id(), by_fqn.shape_id());
    drop(registry);
    by_fqn
}

/// A schema copied out of an entry outlives the entry and its registry.
fn entry_schema_outlives_entry() -> &'static Schema<'static> {
    let registry = TypeRegistry::builder()
        .insert_shape(&prelude::STRING, deserialize_string)
        .build();
    let entry: RegistryEntry = *registry.entry_for_fqn("smithy.api#String").unwrap();
    drop(registry);
    entry.schema()
}

/// Iteration yields static keys whose strings outlive the registry.
fn iteration_keys_are_static() -> Vec<&'static str> {
    let registry = TypeRegistry::builder()
        .insert_shape(&prelude::STRING, deserialize_string)
        .build();
    let keys = registry.iter().map(|(id, _)| id.as_str()).collect();
    drop(registry);
    keys
}

// -- Tests that run the recorded usage ---------------------------------------------------------

#[test]
fn signatures_and_impls_compile() {
    pin_signatures();

    assert_eq!(registry().local_name(), "TypeRegistry");
    assert_eq!(TypeRegistry::builder().local_name(), "TypeRegistryBuilder");
    assert_eq!(ENTRY.local_name(), "RegistryEntry");
    assert_eq!(registry().iter().local_name(), "Iter");
    assert_eq!(
        OPERATION_ERRORS.or(error_registry()).local_name(),
        "ComposedRegistry"
    );
}

#[test]
fn untyped_constructors_infer() {
    let new = TypeRegistry::new();
    let default = TypeRegistry::default();
    let builder = TypeRegistry::builder();
    let builder_default = TypeRegistryBuilder::default();
    let from_trait: TypeRegistry = Default::default();

    assert!(new.is_empty());
    assert!(default.is_empty());
    assert!(builder.build().is_empty());
    assert!(builder_default.build().is_empty());
    assert_eq!(from_trait.len(), 0);
}

#[test]
fn static_lifetimes_are_preserved() {
    assert_eq!(
        schema_outlives_registry().shape_id().as_str(),
        "smithy.api#String"
    );
    assert_eq!(
        entry_schema_outlives_entry().shape_id().as_str(),
        "smithy.api#String"
    );
    assert_eq!(iteration_keys_are_static(), ["smithy.api#String"]);
}

#[test]
fn explicit_insert_and_entry_accessors() {
    let registry = TypeRegistry::builder()
        .insert(shape_id!("smithy.api", "String"), ENTRY)
        .build();
    let entry: &RegistryEntry = registry
        .entry_for(&shape_id!("smithy.api", "String"))
        .unwrap();
    let _: DeserializeFn = entry.deserialize_fn();
    assert!(entry.error_deserialize_fn().is_none());
    assert!(ERROR_ENTRY.error_deserialize_fn().is_some());
}

#[test]
fn iteration_is_exact_size() {
    let mut iter: Iter<'_> = registry().iter();
    assert_eq!(iter.len(), 1);
    let item: Option<(&ShapeId<'static>, &RegistryEntry)> = iter.next();
    assert!(item.is_some());
    assert_eq!(iter.len(), 0);
}

#[test]
fn compose_and_layered_lookup() {
    let base = TypeRegistry::builder()
        .insert_shape(&prelude::STRING, deserialize_string)
        .build();
    let extension = TypeRegistry::builder()
        .insert_shape(&prelude::INTEGER, |d| {
            Ok(TypeErasedBox::new(d.read_integer(&prelude::INTEGER)?))
        })
        .build();
    let composed: TypeRegistry = base.compose(extension);
    assert_eq!(composed.len(), 2);

    let layered: ComposedRegistry<'_> = OPERATION_ERRORS.or(error_registry());
    let copied = layered;
    assert!(layered.entry_for_error_code("String").is_some());
    assert!(copied.entry_for_fqn("smithy.api#String").is_some());
    assert!(copied.entry_for(prelude::STRING.shape_id()).is_some());
}

#[test]
fn deserialize_document_and_reify_error() {
    let document = DiscriminatedDocument::new(Document::String("value".to_string()))
        .with_discriminator("smithy.api#String");
    let value = registry()
        .deserialize_document(&document)
        .unwrap()
        .downcast::<String>()
        .unwrap();
    assert_eq!(*value, "value");

    let body = Document::String("message".to_string());
    let error = reify_error(
        OPERATION_ERRORS.or(error_registry()),
        "String",
        &mut DocumentShapeDeserializer::new(&body),
    )
    .expect("the service error registry reifies the error");
    assert_eq!(error.to_string(), "message");
}
