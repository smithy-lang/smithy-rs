/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! [`TypeRegistry`]s whose runtime entries share the [`RuntimeSchemaSet`] they come from, as
//! a dynamic client's type and error registries would.
//!
//! The registrations own an `Arc` of the set and an owned shape ID, and doesn't borrow. These
//! tests check, through public APIs, that building, indexing, and looking up such
//! registries builds no schemas, that they still deserialize after the caller drops its
//! `Arc` of the set, and that dropping the last of them releases the set.
//!
//! The model doesn't have a prelude, so the tests are quick under Miri:
//!
//! ```text
//! cargo +nightly miri test -p aws-smithy-lang --features runtime-schema --test runtime_type_registry
//! ```

use aws_smithy_lang::runtime_schema::{Materialization, RuntimeSchemaSet};
use aws_smithy_lang::{Model, ShapeId};
use aws_smithy_schema::document::DocumentShapeDeserializer;
use aws_smithy_schema::registry::{
    reify_error, ComposedRegistry, RuntimeRegistration, TypeRegistry,
};
use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
use aws_smithy_schema::Schema;
use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::{DiscriminatedDocument, Document};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::{Arc, Weak};

/// A service with one operation. `ex#NotFound` is the operation's error and
/// `ex#ServiceUnavailable` the service's.
const MODEL: &str = r#"{"smithy": "2.0", "shapes": {
    "ex#Service": {"type": "service", "operations": [{"target": "ex#GetThing"}],
        "errors": [{"target": "ex#ServiceUnavailable"}]},
    "ex#GetThing": {"type": "operation", "input": {"target": "ex#GetThingInput"},
        "output": {"target": "ex#Thing"}, "errors": [{"target": "ex#NotFound"}]},
    "ex#GetThingInput": {"type": "structure", "members": {"id": {"target": "ex#String"}}},
    "ex#Thing": {"type": "structure", "members": {
        "id": {"target": "ex#String"}, "name": {"target": "ex#String"}}},
    "ex#NotFound": {"type": "structure", "members": {"message": {"target": "ex#String"}},
        "traits": {"smithy.api#error": "client"}},
    "ex#ServiceUnavailable": {"type": "structure", "members": {"message": {"target": "ex#String"}},
        "traits": {"smithy.api#error": "server"}},
    "ex#String": {"type": "string"}
}}"#;

fn id(value: &str) -> ShapeId {
    ShapeId::new(value).unwrap()
}

/// A lazy set for `ex#Service`. The model is dropped on return; the set keeps its own copy.
fn schema_set() -> Arc<RuntimeSchemaSet> {
    let model = Model::assembler()
        .disable_prelude()
        .add_json_str("model.json", MODEL)
        .assemble()
        .into_result()
        .unwrap_or_else(|err| panic!("{err}: {:?}", err.diagnostics()));
    let schemas = RuntimeSchemaSet::builder(&model)
        .for_service(&id("ex#Service"))
        .materialization(Materialization::Lazy)
        .build()
        .unwrap_or_else(|err| panic!("{err}"));
    Arc::new(schemas)
}

/// The number of schemas `schemas` has built, which only its `Debug` output reports.
fn built_schemas(schemas: &RuntimeSchemaSet) -> usize {
    let debug = format!("{schemas:?}");
    let count = debug
        .split("built_schemas: ")
        .nth(1)
        .expect("RuntimeSchemaSet's Debug output reports built_schemas");
    count.split(',').next().unwrap().parse().unwrap()
}

/// What the type and error registrations of one shape share.
struct ModelRegistrationData {
    schemas: Arc<RuntimeSchemaSet>,
    id: ShapeId,
}

#[derive(Clone, Copy)]
enum RegistrationKind {
    Type,
    Error,
}

/// A registration of a model shape, as a dynamic client would make one.
///
/// Only an error registration deserializes errors, as only the error registries of a
/// generated client do. A shape's type and error registrations share its data.
struct ModelRegistration {
    data: Arc<ModelRegistrationData>,
    kind: RegistrationKind,
}

fn registration(
    data: &Arc<ModelRegistrationData>,
    kind: RegistrationKind,
) -> Arc<dyn RuntimeRegistration> {
    Arc::new(ModelRegistration {
        data: data.clone(),
        kind,
    })
}

impl ModelRegistration {
    /// Reads a structure whose members are all strings, as every member in `MODEL` is.
    fn read(&self, deserializer: &mut dyn ShapeDeserializer) -> Result<Modeled, SerdeError> {
        let mut members = BTreeMap::new();
        deserializer.read_struct(self.registered_schema(), &mut |member, value| {
            let name = member.member_name().expect("member schemas have names");
            members.insert(name.to_owned(), value.read_string(member)?);
            Ok(())
        })?;
        Ok(Modeled {
            id: self.data.id.clone(),
            members,
        })
    }
}

impl RuntimeRegistration for ModelRegistration {
    fn registered_shape_id(&self) -> &str {
        self.data.id.as_str()
    }

    fn registered_schema(&self) -> &Schema<'_> {
        self.data
            .schemas
            .schema(&self.data.id)
            .expect("registered shapes are in the set")
    }

    fn deserialize_registered(
        &self,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<TypeErasedBox, SerdeError> {
        self.read(deserializer).map(TypeErasedBox::new)
    }

    fn deserialize_registered_error(
        &self,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Option<Box<dyn Error + Send + Sync>>, SerdeError> {
        match self.kind {
            RegistrationKind::Type => Ok(None),
            RegistrationKind::Error => Ok(Some(Box::new(ModeledError(self.read(deserializer)?)))),
        }
    }
}

/// A deserialized structure.
#[derive(Debug, PartialEq)]
struct Modeled {
    id: ShapeId,
    members: BTreeMap<String, String>,
}

/// A deserialized error structure.
#[derive(Debug)]
struct ModeledError(Modeled);

impl fmt::Display for ModeledError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "modeled error {}", self.0.id)
    }
}

impl Error for ModeledError {}

/// The registries a dynamic client would build for `ex#Service`.
struct Registries {
    /// Type registrations of the structures `GetThing` returns.
    types: TypeRegistry,
    /// Error registrations of the service's and the operation's errors.
    service_errors: TypeRegistry,
    /// The operation's error registration, shared with `service_errors`.
    operation_errors: TypeRegistry,
}

impl Registries {
    /// Each shape's registrations share one `Arc` of `schemas`.
    fn new(schemas: &Arc<RuntimeSchemaSet>) -> Self {
        let data = |shape| {
            Arc::new(ModelRegistrationData {
                schemas: schemas.clone(),
                id: id(shape),
            })
        };
        let (thing, not_found, unavailable) = (
            data("ex#Thing"),
            data("ex#NotFound"),
            data("ex#ServiceUnavailable"),
        );
        let not_found_error = registration(&not_found, RegistrationKind::Error);
        Self {
            types: TypeRegistry::builder()
                .extend_shared_runtime(
                    [&thing, &not_found, &unavailable]
                        .map(|data| registration(data, RegistrationKind::Type)),
                )
                .build(),
            service_errors: TypeRegistry::builder()
                .insert_shared_runtime(not_found_error.clone())
                .insert_shared_runtime(registration(&unavailable, RegistrationKind::Error))
                .build(),
            operation_errors: TypeRegistry::builder()
                .insert_shared_runtime(not_found_error)
                .build(),
        }
    }
}

/// A structure document with string members.
fn object<const N: usize>(members: [(&str, &str); N]) -> Document {
    Document::Object(
        members
            .into_iter()
            .map(|(name, value)| (name.to_owned(), Document::String(value.to_owned())))
            .collect(),
    )
}

/// The error `registry` reifies for `code` from `body`, if any.
fn reify(registry: ComposedRegistry<'_>, code: &str, body: &Document) -> Option<Modeled> {
    let error = reify_error(registry, code, &mut DocumentShapeDeserializer::new(body))?;
    let error = error
        .downcast::<ModeledError>()
        .expect("error registrations return ModeledError");
    Some(error.0)
}

#[test]
fn registries_own_their_schema_set() {
    let schemas = schema_set();
    let weak: Weak<RuntimeSchemaSet> = Arc::downgrade(&schemas);
    let Registries {
        types,
        service_errors,
        operation_errors,
    } = Registries::new(&schemas);

    // One `Arc` for each of the three shapes' data, and none held by the registries
    // themselves.
    drop(schemas);
    assert_eq!(weak.strong_count(), 3);

    let thing = DiscriminatedDocument::new(object([("id", "t-1"), ("name", "First")]))
        .with_discriminator("ex#Thing");
    let thing = types
        .deserialize_document(&thing)
        .expect("ex#Thing deserializes");
    assert_eq!(
        *thing.downcast::<Modeled>().unwrap(),
        Modeled {
            id: id("ex#Thing"),
            members: [("id", "t-1"), ("name", "First")]
                .map(|(name, value)| (name.to_owned(), value.to_owned()))
                .into(),
        }
    );

    // The operation's error is in its own registry, and the service's in the fallback.
    let errors = operation_errors.or(&service_errors);
    let body = object([("message", "gone")]);
    for (code, expected) in [
        ("NotFound", "ex#NotFound"),
        ("ex#ServiceUnavailable", "ex#ServiceUnavailable"),
    ] {
        let error = reify(errors, code, &body).unwrap_or_else(|| panic!("{code} reifies"));
        assert_eq!(error.id, expected);
        assert_eq!(error.members["message"], "gone");
    }
    // The type registry's registrations do not deserialize errors.
    assert!(reify(types.or(&TypeRegistry::new()), "NotFound", &body).is_none());

    // `ex#Thing` is only in `types`.
    drop(types);
    assert_eq!(weak.strong_count(), 2);
    // `operation_errors` shares `ex#NotFound`'s error registration with `service_errors`.
    drop(service_errors);
    assert_eq!(weak.strong_count(), 1);
    drop(operation_errors);
    assert!(weak.upgrade().is_none());
}

#[test]
fn registries_build_schemas_only_when_used() {
    let schemas = schema_set();
    let registries = Registries::new(&schemas);
    let errors = registries.operation_errors.or(&registries.service_errors);

    // Building, looking up, iterating, and formatting registries and entries use only the
    // registered shape IDs.
    let thing = registries.types.resolve_fqn("ex#Thing").unwrap();
    assert_eq!(thing.shape_id(), "ex#Thing");
    let unavailable = errors.resolve_error_code("ServiceUnavailable").unwrap();
    assert_eq!(unavailable.shape_id(), "ex#ServiceUnavailable");
    let mut ids: Vec<_> = registries
        .types
        .resolved_entries()
        .map(|(id, _)| id)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["ex#NotFound", "ex#ServiceUnavailable", "ex#Thing"]);
    let _ = format!("{:?} {thing:?} {errors:?}", registries.types);
    assert_eq!(built_schemas(&schemas), 0);

    // An entry's schema is the set's, built on first use.
    let schema = thing.schema();
    assert!(built_schemas(&schemas) > 0);
    assert!(std::ptr::eq(
        schema,
        schemas.schema(&id("ex#Thing")).unwrap()
    ));
}
