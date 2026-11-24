/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime type registry for resolving schemas by `ShapeId`.
//!
//! A [`TypeRegistry`] maps shape IDs to a pair of (a) the corresponding
//! [`Schema`] and (b) a deserialize function that produces a typed value
//! from a [`ShapeDeserializer`].
//!
//! Registries are the runtime piece behind the SEP's "Type Registry" concept.
//! They support two main use cases:
//!
//! 1. **Document → typed shape conversion.** Given a [`Document`](aws_smithy_types::Document) carrying
//!    a discriminator ([`ShapeId`]), the registry resolves the discriminator
//!    to a `RegistryEntry`, walks the document via [`DocumentShapeDeserializer`],
//!    and returns the typed value as a [`TypeErasedBox`] for the caller to
//!    downcast.
//! 2. **Schema lookup by shape ID.** Given a [`ShapeId`] (e.g. one extracted
//!    from an error envelope discriminator), the registry returns the
//!    corresponding [`Schema`] so generated code can dispatch on it.
//!
//! Registries are normally constructed at the package level by code generation
//! and exposed through `Client::registry()` (per the paired SEP). They can
//! also be composed via [`TypeRegistry::compose`] to combine, for example,
//! a service's primary registry with an extension package's registry.
//!
//! See the Smithy SEP "Document Types and Type Registries" for the
//! full specification.

use std::collections::hash_map;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use aws_smithy_types::type_erasure::TypeErasedBox;
use aws_smithy_types::DiscriminatedDocument;

use crate::document::DocumentShapeDeserializer;
use crate::schema::error_envelope::sanitize_error_code;
use crate::serde::{SerdeError, ShapeDeserializer};
use crate::Schema;
use crate::ShapeId;

/// Type-erased deserialize function.
///
/// Takes a [`ShapeDeserializer`] positioned at the value to be read and
/// returns the constructed value as a [`TypeErasedBox`]. The caller
/// downcasts to recover the concrete type.
///
/// Code generation produces one of these per registered shape; the body
/// is typically a thin wrapper around the shape's generated `deserialize`
/// method that wraps the result in `TypeErasedBox::new(...)`.
pub type DeserializeFn = fn(&mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError>;

/// Error-typed deserialize function.
///
/// Like [`DeserializeFn`], but returns the reified value boxed as a
/// `dyn std::error::Error` so it can be attached as the `source` of an
/// unhandled error. Error-registry entries carry one of these in addition to
/// the type-erased [`DeserializeFn`]; primary-registry entries do not.
pub type ErrorDeserializeFn =
    fn(&mut dyn ShapeDeserializer) -> Result<Box<dyn std::error::Error + Send + Sync>, SerdeError>;

/// A single entry in a [`TypeRegistry`], pairing a shape's [`Schema`]
/// with a [`DeserializeFn`] that constructs the typed value.
///
/// Error-registry entries additionally carry an [`ErrorDeserializeFn`] so the
/// reified value can be surfaced as the `source` of an unhandled error; see
/// [`reify_error`].
#[derive(Clone, Copy)]
pub struct RegistryEntry {
    schema: &'static Schema<'static>,
    deserialize: DeserializeFn,
    error_deserialize: Option<ErrorDeserializeFn>,
}

impl RegistryEntry {
    /// Build a `RegistryEntry` from a static schema reference and a
    /// type-erased deserialize function.
    pub const fn new(schema: &'static Schema<'static>, deserialize: DeserializeFn) -> Self {
        Self {
            schema,
            deserialize,
            error_deserialize: None,
        }
    }

    /// Build a `RegistryEntry` for an `@error` shape, carrying both a
    /// type-erased [`DeserializeFn`] and an [`ErrorDeserializeFn`] that boxes
    /// the reified value as a `dyn std::error::Error`.
    pub const fn new_error(
        schema: &'static Schema<'static>,
        deserialize: DeserializeFn,
        error_deserialize: ErrorDeserializeFn,
    ) -> Self {
        Self {
            schema,
            deserialize,
            error_deserialize: Some(error_deserialize),
        }
    }

    /// The static schema for this entry's shape.
    pub fn schema(&self) -> &'static Schema<'static> {
        self.schema
    }

    /// The type-erased deserialize function for this entry.
    pub fn deserialize_fn(&self) -> DeserializeFn {
        self.deserialize
    }

    /// The error deserialize function for this entry, if it was registered as
    /// an `@error` shape.
    pub fn error_deserialize_fn(&self) -> Option<ErrorDeserializeFn> {
        self.error_deserialize
    }
}

impl fmt::Debug for RegistryEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The DeserializeFn pointer is not useful to print; show the shape id.
        f.debug_struct("RegistryEntry")
            .field("shape_id", self.schema.shape_id())
            .finish_non_exhaustive()
    }
}

/// A registry entry that owns or shares its schema and deserialization context.
///
/// A [`RegistryEntry`] requires a `&'static Schema<'static>` and plain function
/// pointers. That suits generated code, but not schemas built at runtime, such as
/// those of a schema set built from a model loaded at runtime. A runtime
/// registration instead returns a schema borrowed from itself, and deserializes
/// with methods that can use its own context.
///
/// Insert registrations with [`TypeRegistryBuilder::insert_runtime`], or, to share
/// one registration between several registries, with
/// [`TypeRegistryBuilder::insert_shared_runtime`]. The registry stores them as
/// `Arc<dyn RuntimeRegistration>`, so a registration must be `'static`: it must
/// own its context or share it through `Arc`, not borrow it. The schema it returns
/// does not need to be `'static`.
///
/// Read runtime entries with the `resolve*` methods of [`TypeRegistry`] and
/// [`ComposedRegistry`], which return a [`ResolvedEntry`]; the methods that return
/// a [`RegistryEntry`] do not see them.
///
/// # Contract
///
/// [`registered_shape_id`](Self::registered_shape_id) must return the shape ID of
/// [`registered_schema`](Self::registered_schema), and must return the same value
/// on every call. A registry indexes a registration by that ID without calling
/// `registered_schema`, so a registration can build its schema on first use.
/// Debug builds check that the two agree each time [`ResolvedEntry::schema`]
/// returns a runtime schema.
///
/// # Example
///
/// ```
/// use aws_smithy_schema::registry::{RuntimeRegistration, TypeRegistry};
/// use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
/// use aws_smithy_schema::{shape_id, Schema, ShapeType};
/// use aws_smithy_types::type_erasure::TypeErasedBox;
/// use aws_smithy_types::{DiscriminatedDocument, Document};
///
/// /// A string shape whose schema and greeting are owned by the registration.
/// struct Greeting {
///     schema: Schema<'static>,
///     greeting: String,
/// }
///
/// impl RuntimeRegistration for Greeting {
///     fn registered_shape_id(&self) -> &str {
///         self.schema.shape_id().as_str()
///     }
///
///     fn registered_schema(&self) -> &Schema<'_> {
///         &self.schema
///     }
///
///     fn deserialize_registered(
///         &self,
///         deserializer: &mut dyn ShapeDeserializer,
///     ) -> Result<TypeErasedBox, SerdeError> {
///         let name = deserializer.read_string(self.registered_schema())?;
///         Ok(TypeErasedBox::new(format!("{}, {name}", self.greeting)))
///     }
/// }
///
/// let registry = TypeRegistry::builder()
///     .insert_runtime(Greeting {
///         schema: Schema::new(shape_id!("example", "Greeting"), ShapeType::String),
///         greeting: "Hello".to_string(),
///     })
///     .build();
///
/// let document = DiscriminatedDocument::new(Document::String("world".to_string()))
///     .with_discriminator("example#Greeting");
/// let value = registry.deserialize_document(&document)?;
/// assert_eq!(*value.downcast::<String>().unwrap(), "Hello, world");
///
/// // The `resolve*` methods see runtime entries; the static lookups do not.
/// assert!(registry.resolve_fqn("example#Greeting").is_some());
/// assert!(registry.schema_for_fqn("example#Greeting").is_none());
/// # Ok::<(), SerdeError>(())
/// ```
pub trait RuntimeRegistration: Send + Sync {
    /// The fully qualified shape ID of the registered shape, such as
    /// `"smithy.example#Foo"`.
    ///
    /// A registry indexes the registration by this ID. It must equal
    /// `self.registered_schema().shape_id().as_str()` and must not change, but
    /// returning it must not require building the schema. See the
    /// [trait documentation](Self#contract).
    fn registered_shape_id(&self) -> &str;

    /// The schema of the registered shape, borrowed from this registration.
    ///
    /// A registration may build the schema on the first call.
    fn registered_schema(&self) -> &Schema<'_>;

    /// Deserializes a value of the registered shape from a deserializer
    /// positioned at the value.
    ///
    /// As with a [`DeserializeFn`], the caller downcasts the returned
    /// [`TypeErasedBox`]; the registration chooses the concrete type. That type
    /// must own its data, because a `TypeErasedBox` holds only `'static` values.
    fn deserialize_registered(
        &self,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<TypeErasedBox, SerdeError>;

    /// Deserializes a value of the registered shape as an error, as an
    /// [`ErrorDeserializeFn`] does for a static entry.
    ///
    /// Returns `Ok(None)` if this is not an error registration, which is what
    /// the default implementation returns, and `Err` if it is one but the value
    /// did not deserialize. [`reify_error`] treats both as no match.
    fn deserialize_registered_error(
        &self,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Option<Box<dyn std::error::Error + Send + Sync>>, SerdeError> {
        let _ = deserializer;
        Ok(None)
    }
}

/// A borrowed view of one registry entry of either kind.
///
/// Returned by the `resolve*` methods of [`TypeRegistry`] and
/// [`ComposedRegistry`], and by [`TypeRegistry::resolved_entries`]. A resolved
/// entry borrows the registry it came from, so its schema cannot outlive that
/// registry. To get a `&'static Schema<'static>` from a static entry, use
/// [`as_static`](Self::as_static).
///
/// # Example
///
/// ```
/// use aws_smithy_schema::document::DocumentShapeDeserializer;
/// use aws_smithy_schema::prelude;
/// use aws_smithy_schema::registry::TypeRegistry;
/// use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
/// use aws_smithy_types::type_erasure::TypeErasedBox;
/// use aws_smithy_types::Document;
///
/// fn deserialize_string(d: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
///     Ok(TypeErasedBox::new(d.read_string(&prelude::STRING)?))
/// }
///
/// let registry = TypeRegistry::builder()
///     .insert_shape(&prelude::STRING, deserialize_string)
///     .build();
///
/// let entry = registry.resolve_fqn("smithy.api#String").unwrap();
/// assert_eq!(entry.shape_id(), "smithy.api#String");
/// assert_eq!(entry.schema().shape_id().as_str(), "smithy.api#String");
/// assert!(entry.as_static().is_some());
///
/// let document = Document::String("value".to_string());
/// let value = entry.deserialize(&mut DocumentShapeDeserializer::new(&document))?;
/// assert_eq!(*value.downcast::<String>().unwrap(), "value");
/// # Ok::<(), SerdeError>(())
/// ```
#[derive(Clone, Copy)]
pub struct ResolvedEntry<'a> {
    /// The key the entry is registered under.
    fqn: &'a str,
    kind: ResolvedEntryKind<'a>,
}

#[derive(Clone, Copy)]
enum ResolvedEntryKind<'a> {
    Static(&'a RegistryEntry),
    Runtime(&'a dyn RuntimeRegistration),
}

impl<'a> ResolvedEntry<'a> {
    fn from_static(id: &'a ShapeId<'static>, entry: &'a RegistryEntry) -> Self {
        Self {
            fqn: id.as_str(),
            kind: ResolvedEntryKind::Static(entry),
        }
    }

    fn from_runtime(fqn: &'a str, registration: &'a Arc<dyn RuntimeRegistration>) -> Self {
        Self {
            fqn,
            kind: ResolvedEntryKind::Runtime(&**registration),
        }
    }

    /// The fully qualified shape ID the entry is registered under.
    ///
    /// This does not build a runtime entry's schema.
    pub fn shape_id(&self) -> &'a str {
        self.fqn
    }

    /// The entry's schema.
    ///
    /// For a runtime entry, this calls
    /// [`RuntimeRegistration::registered_schema`], which may build the schema.
    pub fn schema(&self) -> &'a Schema<'a> {
        match self.kind {
            ResolvedEntryKind::Static(entry) => entry.schema(),
            ResolvedEntryKind::Runtime(registration) => {
                let schema = registration.registered_schema();
                debug_assert_eq!(
                    schema.shape_id().as_str(),
                    self.fqn,
                    "a RuntimeRegistration's schema must have its registered_shape_id"
                );
                schema
            }
        }
    }

    /// Deserializes a value of the entry's shape from a deserializer positioned
    /// at the value. The caller downcasts the returned [`TypeErasedBox`].
    pub fn deserialize(
        &self,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<TypeErasedBox, SerdeError> {
        match self.kind {
            ResolvedEntryKind::Static(entry) => (entry.deserialize)(deserializer),
            ResolvedEntryKind::Runtime(registration) => {
                registration.deserialize_registered(deserializer)
            }
        }
    }

    /// Deserializes a value of the entry's shape as an error.
    ///
    /// Returns `Ok(None)` if the entry was not registered as an error, and
    /// `Err` if it was but the value did not deserialize.
    pub fn deserialize_error(
        &self,
        deserializer: &mut dyn ShapeDeserializer,
    ) -> Result<Option<Box<dyn std::error::Error + Send + Sync>>, SerdeError> {
        match self.kind {
            ResolvedEntryKind::Static(entry) => entry
                .error_deserialize
                .map(|error_deserialize| error_deserialize(deserializer))
                .transpose(),
            ResolvedEntryKind::Runtime(registration) => {
                registration.deserialize_registered_error(deserializer)
            }
        }
    }

    /// The static entry, or `None` for a runtime entry.
    pub fn as_static(&self) -> Option<&'a RegistryEntry> {
        match self.kind {
            ResolvedEntryKind::Static(entry) => Some(entry),
            ResolvedEntryKind::Runtime(_) => None,
        }
    }

    /// The runtime registration, or `None` for a static entry.
    pub fn as_runtime(&self) -> Option<&'a dyn RuntimeRegistration> {
        match self.kind {
            ResolvedEntryKind::Static(_) => None,
            ResolvedEntryKind::Runtime(registration) => Some(registration),
        }
    }
}

impl fmt::Debug for ResolvedEntry<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Does not call `schema()`, which could build a runtime schema.
        let kind = match self.kind {
            ResolvedEntryKind::Static(_) => "static",
            ResolvedEntryKind::Runtime(_) => "runtime",
        };
        f.debug_struct("ResolvedEntry")
            .field("shape_id", &self.fqn)
            .field("kind", &kind)
            .finish()
    }
}

/// The runtime entries of a [`TypeRegistry`], keyed by fully qualified shape ID.
///
/// A registry boxes this and allocates it only when a runtime entry is inserted,
/// so a registry with only static entries pays one pointer for it.
#[derive(Default)]
struct RuntimeEntries {
    // Keys are owned copies of `registered_shape_id`. This crate's `ShapeId<'_>`,
    // like `&str`, only borrows its strings, and the registration this map holds
    // is their only owner, so a borrowed key would be self-referential. (The owned
    // `aws_smithy_lang::ShapeId` is in a crate that depends on this one.) Keying
    // by the registration itself would save this allocation, but every lookup
    // would then make a virtual call, and an ID that changes would corrupt the map.
    by_fqn: HashMap<Box<str>, Arc<dyn RuntimeRegistration>>,
}

impl RuntimeEntries {
    fn resolve(&self, fqn: &str) -> Option<ResolvedEntry<'_>> {
        self.by_fqn
            .get_key_value(fqn)
            .map(|(fqn, registration)| ResolvedEntry::from_runtime(fqn, registration))
    }

    /// The first entry whose shape name is `shape_name`, as
    /// [`TypeRegistry::entry_for_error_code`] matches static entries.
    fn resolve_shape_name(&self, shape_name: &str) -> Option<ResolvedEntry<'_>> {
        self.by_fqn.iter().find_map(|(fqn, registration)| {
            (shape_name_of(fqn) == shape_name)
                .then(|| ResolvedEntry::from_runtime(fqn, registration))
        })
    }
}

/// The shape name of a fully qualified shape ID, as [`ShapeId::shape_name`]
/// returns it: the part after `#`, without a `$member` suffix.
fn shape_name_of(fqn: &str) -> &str {
    let name = fqn.split_once('#').map_or(fqn, |(_, name)| name);
    name.split_once('$').map_or(name, |(name, _)| name)
}

/// A runtime mapping of [`ShapeId`] to registry entries.
///
/// A registry holds static entries ([`RegistryEntry`]) and runtime entries
/// ([`RuntimeRegistration`]); see the [module documentation](crate::registry)
/// for which methods see which. Construct via [`TypeRegistry::builder`] for the
/// most common pattern, or [`TypeRegistry::new`]/[`Default::default`] for an
/// empty registry.
///
/// Use [`TypeRegistry::compose`] to merge two registries; entries in the
/// argument registry override entries with the same shape ID in `self`.
///
/// # Cross-lifetime lookup
///
/// Static entries are keyed by `ShapeId<'static>` (codegen-emitted shapes
/// always have static lifetime), and runtime entries by an owned copy of their
/// shape ID. The public lookup methods accept `&ShapeId<'_>` of any lifetime;
/// lookup is by fully qualified name via `ShapeId`'s `Borrow<str>` impl. The
/// companion `*_fqn` methods take `&str` directly — handy after extracting a
/// `__type` field from a wire-format JSON document.
#[derive(Default)]
pub struct TypeRegistry {
    entries: HashMap<ShapeId<'static>, RegistryEntry>,
    /// The runtime entries, or `None` if there are none. No shape ID is a key
    /// of both maps.
    runtime: Option<Box<RuntimeEntries>>,
}

impl TypeRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start building a registry. See [`TypeRegistryBuilder`].
    pub fn builder() -> TypeRegistryBuilder {
        TypeRegistryBuilder::default()
    }

    /// Look up the static schema for `id`, or `None` if not registered.
    ///
    /// Accepts a [`ShapeId`] of any lifetime — lookup is by fully
    /// qualified name. `ShapeId<'static>` (codegen-emitted) and a
    /// runtime-built `ShapeId<'_>` with the same FQN resolve to the
    /// same entry.
    ///
    /// Sees only static entries; use [`Self::resolve`] to include runtime
    /// entries.
    pub fn schema_for(&self, id: &ShapeId<'_>) -> Option<&'static Schema<'static>> {
        self.entries.get(id.as_str()).map(|e| e.schema)
    }

    /// Look up the static schema for the given fully qualified name
    /// (e.g. `"smithy.example#Bird"`).
    ///
    /// Use this when you already have the FQN as a string slice — for
    /// instance, after extracting a `__type` discriminator from a
    /// JSON document.
    ///
    /// # Example
    ///
    /// ```
    /// use aws_smithy_schema::prelude;
    /// use aws_smithy_schema::registry::TypeRegistry;
    /// use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
    /// use aws_smithy_types::type_erasure::TypeErasedBox;
    ///
    /// fn deserialize_string(d: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
    ///     Ok(TypeErasedBox::new(d.read_string(&prelude::STRING)?))
    /// }
    ///
    /// let registry = TypeRegistry::builder()
    ///     .insert_shape(&prelude::STRING, deserialize_string)
    ///     .build();
    ///
    /// // FQN extracted at runtime — for instance from a wire-format
    /// // `__type` field — looks up against the static registry.
    /// assert!(registry.schema_for_fqn("smithy.api#String").is_some());
    /// assert!(registry.schema_for_fqn("smithy.api#Boolean").is_none());
    /// ```
    ///
    /// Sees only static entries; use [`Self::resolve_fqn`] to include runtime
    /// entries.
    pub fn schema_for_fqn(&self, fqn: &str) -> Option<&'static Schema<'static>> {
        self.entries.get(fqn).map(|e| e.schema)
    }

    /// Look up the entry (schema + deserialize fn) for `id`, or `None`
    /// if not registered.
    ///
    /// Accepts a [`ShapeId`] of any lifetime; see [`Self::schema_for`].
    ///
    /// Sees only static entries; use [`Self::resolve`] to include runtime
    /// entries.
    pub fn entry_for(&self, id: &ShapeId<'_>) -> Option<&RegistryEntry> {
        self.entries.get(id.as_str())
    }

    /// Look up the entry for the given fully qualified name. See
    /// [`Self::schema_for_fqn`].
    ///
    /// Sees only static entries; use [`Self::resolve_fqn`] to include runtime
    /// entries.
    pub fn entry_for_fqn(&self, fqn: &str) -> Option<&RegistryEntry> {
        self.entries.get(fqn)
    }

    /// Look up the entry for an error shape by its **wire error code**
    /// (e.g. `"NotFound"`), rather than by fully qualified name.
    ///
    /// Error responses identify the error by a bare code on the wire — the
    /// `<Code>` element for restXml, the `__type` discriminator for the JSON
    /// protocols, the `x-amzn-query-error` header for awsQueryCompatible — not
    /// by a fully qualified [`ShapeId`]. This resolves such a code to its entry
    /// so a schema-serde error path can reify an error by the identifier it
    /// actually receives off the wire.
    ///
    /// `wire_code` is sanitized first (a leading `namespace#` prefix and a
    /// trailing `:url` suffix are stripped), so a bare name, a fully qualified
    /// name, or a URL-suffixed code all resolve correctly. Matching is by the
    /// registered shape's name; per-operation and per-service error sets are
    /// small, so a linear scan is sufficient and needs no extra generated index.
    ///
    /// Returns `None` if no registered shape's name matches the code.
    ///
    /// Sees only static entries; use [`Self::resolve_error_code`] to include
    /// runtime entries.
    //
    // TODO(schema-error): honor an `@awsQueryError` code override once `Schema`
    // carries that trait — awsQueryCompatible services can emit a custom error
    // code that differs from the shape name, which this name-based match would miss.
    pub fn entry_for_error_code(&self, wire_code: &str) -> Option<&RegistryEntry> {
        let wire_code = sanitize_error_code(wire_code);
        self.iter()
            .find_map(|(id, entry)| (id.shape_name() == wire_code).then_some(entry))
    }

    /// Look up the entry of either kind for `id`, or `None` if not registered.
    ///
    /// Accepts a [`ShapeId`] of any lifetime; see [`Self::resolve_fqn`].
    pub fn resolve(&self, id: &ShapeId<'_>) -> Option<ResolvedEntry<'_>> {
        self.resolve_fqn(id.as_str())
    }

    /// Look up the entry of either kind for the given fully qualified name,
    /// or `None` if not registered.
    ///
    /// This does not build a runtime entry's schema. See [`ResolvedEntry`] for
    /// an example.
    pub fn resolve_fqn(&self, fqn: &str) -> Option<ResolvedEntry<'_>> {
        // A registry with entries of only one kind hashes `fqn` once.
        match self.runtime.as_deref() {
            None => self.resolve_static(fqn),
            Some(runtime) if self.entries.is_empty() => runtime.resolve(fqn),
            Some(runtime) => self.resolve_static(fqn).or_else(|| runtime.resolve(fqn)),
        }
    }

    fn resolve_static(&self, fqn: &str) -> Option<ResolvedEntry<'_>> {
        self.entries
            .get_key_value(fqn)
            .map(|(id, entry)| ResolvedEntry::from_static(id, entry))
    }

    /// Look up the entry of either kind for an error shape by its wire error
    /// code, or `None` if no registered shape's name matches the code.
    ///
    /// The code is sanitized and matched by shape name as in
    /// [`Self::entry_for_error_code`]. This does not build a runtime entry's
    /// schema.
    ///
    /// Shape IDs are unique within a registry, but shape names are not. If two
    /// entries share the shape name the code matches, which one this returns is
    /// unspecified. To give one set of errors precedence over another, layer
    /// their registries with [`Self::or`].
    //
    // TODO(schema-error): honor an `@awsQueryError` code override here too, as
    // noted on `entry_for_error_code`.
    pub fn resolve_error_code(&self, wire_code: &str) -> Option<ResolvedEntry<'_>> {
        let shape_name = sanitize_error_code(wire_code);
        let resolve_static = || {
            self.entries.iter().find_map(|(id, entry)| {
                (id.shape_name() == shape_name).then(|| ResolvedEntry::from_static(id, entry))
            })
        };
        match self.runtime.as_deref() {
            None => resolve_static(),
            Some(runtime) if self.entries.is_empty() => runtime.resolve_shape_name(shape_name),
            Some(runtime) => resolve_static().or_else(|| runtime.resolve_shape_name(shape_name)),
        }
    }

    /// Deserialize the given `document` into the shape pointed to by its
    /// discriminator.
    ///
    /// Returns `Err(SerdeError::InvalidInput { .. })` if the document has no
    /// discriminator, and `Err(SerdeError::UnknownMember { .. })` if the
    /// discriminator does not match a registered shape. Entries of both kinds
    /// are considered.
    ///
    /// On success, the returned [`TypeErasedBox`] carries an instance of the
    /// concrete data type that the entry's [`DeserializeFn`] or
    /// [`RuntimeRegistration::deserialize_registered`] produces. Callers
    /// downcast via [`TypeErasedBox::downcast`].
    ///
    /// The document's lifetime is independent of the registry's storage —
    /// runtime-parsed documents (e.g. with a `__type` discriminator
    /// borrowed from input bytes) resolve against the `'static`-keyed
    /// registry via FQN matching.
    ///
    /// # Example
    ///
    /// ```
    /// use aws_smithy_schema::prelude;
    /// use aws_smithy_schema::registry::TypeRegistry;
    /// use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
    /// use aws_smithy_types::{DiscriminatedDocument, Document};
    /// use aws_smithy_types::type_erasure::TypeErasedBox;
    ///
    /// fn deserialize_string(d: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
    ///     Ok(TypeErasedBox::new(d.read_string(&prelude::STRING)?))
    /// }
    ///
    /// let registry = TypeRegistry::builder()
    ///     .insert_shape(&prelude::STRING, deserialize_string)
    ///     .build();
    ///
    /// // Build a discriminated document. In practice this typically comes
    /// // from `JsonDeserializer::read_discriminated_document` after a
    /// // wire-format `__type` lift.
    /// let doc = DiscriminatedDocument::new(Document::String("hi".to_string()))
    ///     .with_discriminator("smithy.api#String");
    ///
    /// let typed = registry.deserialize_document(&doc).unwrap();
    /// let value = typed.downcast::<String>().unwrap();
    /// assert_eq!(*value, "hi");
    /// ```
    pub fn deserialize_document(
        &self,
        document: &DiscriminatedDocument,
    ) -> Result<TypeErasedBox, SerdeError> {
        let id = document.discriminator().ok_or_else(|| {
            SerdeError::invalid_input(
                "document has no discriminator; cannot resolve shape via TypeRegistry".to_string(),
            )
        })?;
        let entry = self
            .resolve_fqn(id)
            .ok_or_else(|| SerdeError::unknown_member(id.to_string()))?;
        let mut deser = DocumentShapeDeserializer::new_with_settings(
            document.document(),
            document.settings().cloned(),
        );
        entry.deserialize(&mut deser)
    }

    /// Combine `self` and `other` into a new registry containing the union
    /// of their entries.
    ///
    /// On shape ID collision, entries from `other` override entries in
    /// `self`, whether static or runtime defined. This matches the typical use
    /// case where the caller wants extension-package entries to take precedence
    /// over base-package entries.
    pub fn compose(mut self, other: TypeRegistry) -> TypeRegistry {
        let TypeRegistry { entries, runtime } = other;
        self.entries.reserve(entries.len());
        for (id, entry) in entries {
            self.insert_static_entry(id, entry);
        }
        // `other` has no shape ID in both maps, so moving its static entries
        // before its runtime entries cannot change the result.
        if let Some(runtime) = runtime {
            self.runtime
                .get_or_insert_with(Box::default)
                .by_fqn
                .reserve(runtime.by_fqn.len());
            for (fqn, registration) in runtime.by_fqn {
                self.insert_runtime_entry_keyed(fqn, registration);
            }
        }
        self
    }

    /// Borrow `self` and `fallback` into a [`ComposedRegistry`] whose lookups
    /// consult `self` first and fall back to `fallback`.
    ///
    /// Unlike [`compose`](Self::compose), this neither consumes nor copies
    /// either registry — both are borrowed in place, so two `&'static`
    /// registries (the typical case for codegen-emitted statics) can be
    /// layered per call with no allocation.
    ///
    /// The precedence — `self` wins over `fallback` on a colliding lookup — is
    /// what makes this useful for **operation-scoped** error resolution: layer
    /// an operation's own error registry over the service-wide one so the
    /// operation's modeled errors take priority before widening to the rest of
    /// the service closure.
    pub fn or<'a>(&'a self, fallback: &'a TypeRegistry) -> ComposedRegistry<'a> {
        ComposedRegistry {
            primary: self,
            fallback,
        }
    }

    /// Iterate the registry's static `(ShapeId, RegistryEntry)` pairs.
    /// Iteration order is unspecified.
    ///
    /// Runtime entries are not included; use [`Self::resolved_entries`] to
    /// iterate entries of both kinds.
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            inner: self.entries.iter(),
        }
    }

    /// Iterate every entry of either kind with its fully qualified shape ID.
    ///
    /// Static entries come first, then runtime entries; the order within each
    /// kind is unspecified. This does not build any runtime entry's schema.
    pub fn resolved_entries(&self) -> ResolvedIter<'_> {
        ResolvedIter {
            statics: self.entries.iter(),
            runtime: self.runtime.as_ref().map(|runtime| runtime.by_fqn.iter()),
        }
    }

    /// Number of registered entries of either kind.
    ///
    /// For a registry with runtime entries, this is more than
    /// `self.iter().len()`, which counts only static entries.
    pub fn len(&self) -> usize {
        self.entries.len() + self.runtime_len()
    }

    /// `true` iff the registry has no entries of either kind.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty() && self.runtime_len() == 0
    }

    fn runtime_len(&self) -> usize {
        self.runtime
            .as_ref()
            .map_or(0, |runtime| runtime.by_fqn.len())
    }

    /// Inserts a static entry, replacing any entry of either kind with the same
    /// shape ID.
    fn insert_static_entry(&mut self, id: ShapeId<'static>, entry: RegistryEntry) {
        if let Some(runtime) = self.runtime.as_mut() {
            if runtime.by_fqn.remove(id.as_str()).is_some() && runtime.by_fqn.is_empty() {
                self.runtime = None;
            }
        }
        self.entries.insert(id, entry);
    }

    /// Inserts a runtime entry under its registered shape ID, replacing any
    /// entry of either kind with the same shape ID. Does not call
    /// `registered_schema`.
    fn insert_runtime_entry(&mut self, registration: Arc<dyn RuntimeRegistration>) {
        let fqn = registration.registered_shape_id().into();
        self.insert_runtime_entry_keyed(fqn, registration);
    }

    fn insert_runtime_entry_keyed(
        &mut self,
        fqn: Box<str>,
        registration: Arc<dyn RuntimeRegistration>,
    ) {
        self.entries.remove(&*fqn);
        self.runtime
            .get_or_insert_with(Box::default)
            .by_fqn
            .insert(fqn, registration);
    }

    /// Frees the runtime map if it has no entries, so a registry without
    /// runtime entries never holds one.
    fn drop_empty_runtime(&mut self) {
        if self.runtime_len() == 0 {
            self.runtime = None;
        }
    }

    fn debug_counts(&self, name: &str, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Counts only, so formatting never builds a runtime schema.
        f.debug_struct(name)
            .field("entries", &self.len())
            .field("static_entries", &self.entries.len())
            .field("runtime_entries", &self.runtime_len())
            .finish_non_exhaustive()
    }
}

impl fmt::Debug for TypeRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_counts("TypeRegistry", f)
    }
}

/// Two [`TypeRegistry`] references layered for lookup, produced by
/// [`TypeRegistry::or`].
///
/// Lookups consult the `primary` registry first and fall back to the
/// `fallback` registry, so an entry in `primary` shadows an entry with the
/// same key in `fallback`. Both registries are borrowed, so a `ComposedRegistry`
/// is a cheap, allocation-free view — typically a per-operation error registry
/// layered over the service-wide one for operation-scoped error resolution.
#[derive(Debug, Clone, Copy)]
pub struct ComposedRegistry<'a> {
    primary: &'a TypeRegistry,
    fallback: &'a TypeRegistry,
}

impl<'a> ComposedRegistry<'a> {
    /// Look up a static entry by [`ShapeId`], consulting `primary` then
    /// `fallback`.
    ///
    /// Sees only static entries; use [`Self::resolve`] to include runtime
    /// entries.
    pub fn entry_for(&self, id: &ShapeId<'_>) -> Option<&RegistryEntry> {
        self.primary
            .entry_for(id)
            .or_else(|| self.fallback.entry_for(id))
    }

    /// Look up a static entry by fully qualified name, consulting `primary`
    /// then `fallback`.
    ///
    /// Sees only static entries; use [`Self::resolve_fqn`] to include runtime
    /// entries.
    pub fn entry_for_fqn(&self, fqn: &str) -> Option<&RegistryEntry> {
        self.primary
            .entry_for_fqn(fqn)
            .or_else(|| self.fallback.entry_for_fqn(fqn))
    }

    /// Look up a static entry by wire error code, consulting `primary` then
    /// `fallback`. See [`TypeRegistry::entry_for_error_code`].
    ///
    /// This is the operation-scoped error resolution primitive: with an
    /// operation's error registry as `primary` and the service-wide registry
    /// as `fallback`, an error the operation models is resolved against the
    /// operation's own set before the lookup widens to the rest of the service.
    ///
    /// Sees only static entries; use [`Self::resolve_error_code`] to include
    /// runtime entries.
    pub fn entry_for_error_code(&self, wire_code: &str) -> Option<&RegistryEntry> {
        self.primary
            .entry_for_error_code(wire_code)
            .or_else(|| self.fallback.entry_for_error_code(wire_code))
    }

    /// Look up an entry of either kind by [`ShapeId`], consulting `primary`
    /// then `fallback`.
    ///
    /// The result borrows the layered registries, not this view, so it can
    /// outlive a temporary `primary.or(&fallback)`.
    pub fn resolve(&self, id: &ShapeId<'_>) -> Option<ResolvedEntry<'a>> {
        self.resolve_fqn(id.as_str())
    }

    /// Look up an entry of either kind by fully qualified name, consulting
    /// `primary` then `fallback`.
    pub fn resolve_fqn(&self, fqn: &str) -> Option<ResolvedEntry<'a>> {
        self.primary
            .resolve_fqn(fqn)
            .or_else(|| self.fallback.resolve_fqn(fqn))
    }

    /// Look up an entry of either kind by wire error code, consulting
    /// `primary` then `fallback`. See [`TypeRegistry::resolve_error_code`].
    ///
    /// The precedence holds whatever the kinds of the entries, so a runtime
    /// operation error registry can be layered over a generated service error
    /// registry, or the reverse:
    ///
    /// ```
    /// use aws_smithy_schema::prelude;
    /// use aws_smithy_schema::registry::{RuntimeRegistration, TypeRegistry};
    /// use aws_smithy_schema::serde::{SerdeError, ShapeDeserializer};
    /// use aws_smithy_schema::{shape_id, Schema, ShapeType};
    /// use aws_smithy_types::type_erasure::TypeErasedBox;
    /// # struct Owned(Schema<'static>);
    /// # impl RuntimeRegistration for Owned {
    /// #     fn registered_shape_id(&self) -> &str { self.0.shape_id().as_str() }
    /// #     fn registered_schema(&self) -> &Schema<'_> { &self.0 }
    /// #     fn deserialize_registered(
    /// #         &self,
    /// #         d: &mut dyn ShapeDeserializer,
    /// #     ) -> Result<TypeErasedBox, SerdeError> {
    /// #         Ok(TypeErasedBox::new(d.read_string(&self.0)?))
    /// #     }
    /// # }
    /// # fn deserialize_string(d: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
    /// #     Ok(TypeErasedBox::new(d.read_string(&prelude::STRING)?))
    /// # }
    ///
    /// // A generated service registry with `smithy.api#String`, and a runtime
    /// // operation registry with `example#String`. Both have the shape name
    /// // `String`.
    /// let service = TypeRegistry::builder()
    ///     .insert_shape(&prelude::STRING, deserialize_string)
    ///     .build();
    /// let operation = TypeRegistry::builder()
    ///     .insert_runtime(Owned(Schema::new(
    ///         shape_id!("example", "String"),
    ///         ShapeType::String,
    ///     )))
    ///     .build();
    ///
    /// let entry = operation.or(&service).resolve_error_code("String").unwrap();
    /// assert_eq!(entry.shape_id(), "example#String");
    /// assert!(entry.as_runtime().is_some());
    /// ```
    pub fn resolve_error_code(&self, wire_code: &str) -> Option<ResolvedEntry<'a>> {
        self.primary
            .resolve_error_code(wire_code)
            .or_else(|| self.fallback.resolve_error_code(wire_code))
    }
}

/// Reify an error response body into a boxed `dyn std::error::Error`, scoped to
/// an operation and then widened to the service.
///
/// Given a `registry` composed as `operation_errors.or(&service_errors)`, a wire
/// `error_code`, and a [`ShapeDeserializer`] positioned at the error body (such
/// as the one returned by a protocol's `deserialize_error_response`), this looks
/// the code up — the operation's own errors first, then the service-wide set —
/// and runs the matched entry's [`ErrorDeserializeFn`], or for a runtime entry,
/// its [`RuntimeRegistration::deserialize_registered_error`]. The result is the
/// typed error boxed as a `dyn std::error::Error`, ready to be attached as the
/// `source` of an unhandled error.
///
/// It is the registry-backed fallback for the schema-serde error path, meant to
/// run only after an operation's modeled-error match misses, to recover
/// structure from an error the operation does not model but the service (or an
/// extension package) does. Deserialization failures are swallowed (returning
/// `None`) so a malformed or unexpected body can never change how an operation
/// fails relative to the plain generic-error path.
///
/// HTTP response bindings on a reified error *are* populated, provided the
/// caller passes a deserializer obtained from a protocol that owns them. A REST
/// protocol's `deserialize_error_response` returns a composite that reads
/// `@httpHeader`, `@httpPrefixHeaders`, and `@httpResponseCode` members from the
/// response and delegates the remaining members to the body codec, so the
/// entry's [`ErrorDeserializeFn`] — which is the shape's protocol-agnostic
/// member consumer — receives values from both sources. Body-only protocols
/// return a body-only deserializer and those members stay absent, which is the
/// correct behavior for them.
///
/// Returns `None` if `error_code` matches no registered shape, if the matched
/// entry was not registered as an `@error` shape (a static entry with no
/// [`ErrorDeserializeFn`], or a runtime entry whose error deserializer returns
/// `Ok(None)`), or if deserialization of the matched shape fails.
pub fn reify_error(
    registry: ComposedRegistry<'_>,
    error_code: &str,
    deser: &mut dyn ShapeDeserializer,
) -> Option<Box<dyn std::error::Error + Send + Sync>> {
    let entry = registry.resolve_error_code(error_code)?;
    entry.deserialize_error(deser).ok().flatten()
}

/// Builder for [`TypeRegistry`].
///
/// Each insertion replaces any entry of either kind with the same shape ID, so
/// the last insertion for a shape ID wins.
#[derive(Default)]
pub struct TypeRegistryBuilder {
    registry: TypeRegistry,
}

impl fmt::Debug for TypeRegistryBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.registry.debug_counts("TypeRegistryBuilder", f)
    }
}

impl TypeRegistryBuilder {
    /// Insert an explicit `(ShapeId, RegistryEntry)` pair.
    pub fn insert(mut self, id: ShapeId<'static>, entry: RegistryEntry) -> Self {
        self.registry.insert_static_entry(id, entry);
        self
    }

    /// Insert a registration derived from a schema and a deserialize
    /// function. The shape ID is taken from the schema.
    ///
    /// This is the form generated code typically uses.
    pub fn insert_shape(
        mut self,
        schema: &'static Schema<'static>,
        deserialize: DeserializeFn,
    ) -> Self {
        let id = schema.shape_id().clone();
        self.registry
            .insert_static_entry(id, RegistryEntry::new(schema, deserialize));
        self
    }

    /// Insert a registration for an `@error` shape, carrying both a type-erased
    /// [`DeserializeFn`] and an [`ErrorDeserializeFn`]. The shape ID is taken
    /// from the schema.
    ///
    /// This is the form generated code uses for error registries; the
    /// [`ErrorDeserializeFn`] lets [`reify_error`] surface the reified value as
    /// the `source` of an unhandled error.
    pub fn insert_error_shape(
        mut self,
        schema: &'static Schema<'static>,
        deserialize: DeserializeFn,
        error_deserialize: ErrorDeserializeFn,
    ) -> Self {
        let id = schema.shape_id().clone();
        self.registry.insert_static_entry(
            id,
            RegistryEntry::new_error(schema, deserialize, error_deserialize),
        );
        self
    }

    /// Insert a runtime registration under its
    /// [`registered_shape_id`](RuntimeRegistration::registered_shape_id).
    ///
    /// This does not call
    /// [`registered_schema`](RuntimeRegistration::registered_schema). To insert
    /// one registration into several registries, use
    /// [`insert_shared_runtime`](Self::insert_shared_runtime).
    pub fn insert_runtime(self, registration: impl RuntimeRegistration + 'static) -> Self {
        self.insert_shared_runtime(Arc::new(registration))
    }

    /// Insert a shared runtime registration under its
    /// [`registered_shape_id`](RuntimeRegistration::registered_shape_id).
    ///
    /// Share a registration this way when the same shape belongs in several
    /// registries, such as an error in a service's error registry and in the
    /// error registries of the operations that model it.
    pub fn insert_shared_runtime(mut self, registration: Arc<dyn RuntimeRegistration>) -> Self {
        self.registry.insert_runtime_entry(registration);
        self
    }

    /// Insert several shared runtime registrations, as
    /// [`insert_shared_runtime`](Self::insert_shared_runtime) does, reserving
    /// space for them first.
    pub fn extend_shared_runtime(
        mut self,
        registrations: impl IntoIterator<Item = Arc<dyn RuntimeRegistration>>,
    ) -> Self {
        let registrations = registrations.into_iter();
        let (additional, _) = registrations.size_hint();
        if additional > 0 {
            self.registry
                .runtime
                .get_or_insert_with(Box::default)
                .by_fqn
                .reserve(additional);
        }
        for registration in registrations {
            self.registry.insert_runtime_entry(registration);
        }
        self
    }

    /// Finalize the builder into a [`TypeRegistry`].
    pub fn build(self) -> TypeRegistry {
        let mut registry = self.registry;
        // `extend_shared_runtime` allocates the runtime map from a size hint,
        // before knowing whether anything is inserted.
        registry.drop_empty_runtime();
        registry
    }
}

/// Iterator over the static entries of a [`TypeRegistry`]. Returned by
/// [`TypeRegistry::iter`].
pub struct Iter<'a> {
    inner: hash_map::Iter<'a, ShapeId<'static>, RegistryEntry>,
}

impl fmt::Debug for Iter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter").finish_non_exhaustive()
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a ShapeId<'static>, &'a RegistryEntry);

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for Iter<'_> {
    fn len(&self) -> usize {
        self.inner.len()
    }
}

/// Iterator over the entries of either kind of a [`TypeRegistry`], with their
/// fully qualified shape IDs. Returned by [`TypeRegistry::resolved_entries`].
pub struct ResolvedIter<'a> {
    statics: hash_map::Iter<'a, ShapeId<'static>, RegistryEntry>,
    runtime: Option<hash_map::Iter<'a, Box<str>, Arc<dyn RuntimeRegistration>>>,
}

impl fmt::Debug for ResolvedIter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResolvedIter").finish_non_exhaustive()
    }
}

impl<'a> Iterator for ResolvedIter<'a> {
    type Item = (&'a str, ResolvedEntry<'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if let Some((id, entry)) = self.statics.next() {
            return Some((id.as_str(), ResolvedEntry::from_static(id, entry)));
        }
        let (fqn, registration) = self.runtime.as_mut()?.next()?;
        Some((fqn, ResolvedEntry::from_runtime(fqn, registration)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for ResolvedIter<'_> {
    fn len(&self) -> usize {
        self.statics.len() + self.runtime.as_ref().map_or(0, |runtime| runtime.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use aws_smithy_types::{Blob, Document, DocumentError, DocumentSettings, Number};

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::shape_id;
    use crate::ShapeType;

    // -- Schemas used across tests --------------------------------------------------------------

    static M_FOO_NAME: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Foo", "name"),
        ShapeType::String,
        "name",
        0,
    );
    static FOO_SCHEMA: Schema<'static> = Schema::new_struct(
        shape_id!("smithy.example", "Foo"),
        ShapeType::Structure,
        &[&M_FOO_NAME],
    );

    static M_BAR_VALUE: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Bar", "value"),
        ShapeType::Integer,
        "value",
        0,
    );
    static BAR_SCHEMA: Schema<'static> = Schema::new_struct(
        shape_id!("smithy.example", "Bar"),
        ShapeType::Structure,
        &[&M_BAR_VALUE],
    );

    // A structure with a `blob` member, used to verify that protocol
    // settings are threaded through `deserialize_document` so wire-encoded
    // blobs (JSON base64 strings) coerce on the type-registry path.
    static M_WIDGET_DATA: Schema<'static> = Schema::new_member(
        shape_id!("smithy.example", "Widget", "data"),
        ShapeType::Blob,
        "data",
        0,
    );
    static WIDGET_SCHEMA: Schema<'static> = Schema::new_struct(
        shape_id!("smithy.example", "Widget"),
        ShapeType::Structure,
        &[&M_WIDGET_DATA],
    );

    // -- Test assertion helpers -----------------------------------------------------------------

    /// Assert that `actual` is the same function as `expected`.
    ///
    /// This is a no-op under Miri. Miri does not guarantee that a function has a
    /// single unique address, so two separate reifications of the same `fn` item
    /// may compare unequal and `fn_addr_eq` would spuriously fail. Gating only
    /// the comparison (rather than `#[cfg_attr(miri, ignore)]` on the whole test)
    /// keeps the surrounding assertions running under Miri.
    fn assert_same_fn(actual: DeserializeFn, expected: DeserializeFn) {
        #[cfg(not(miri))]
        assert!(
            std::ptr::fn_addr_eq(actual, expected),
            "expected the registered deserialize fn"
        );
        #[cfg(miri)]
        let _ = (actual, expected);
    }

    // -- Test types and their deserialize fns ---------------------------------------------------

    #[derive(Debug, PartialEq)]
    struct Foo {
        name: Option<String>,
    }

    fn deserialize_foo(deser: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
        let mut out = Foo { name: None };
        deser.read_struct(&FOO_SCHEMA, &mut |member, sub| {
            if let Some(0) = member.member_index() {
                out.name = Some(sub.read_string(member)?);
            }
            Ok(())
        })?;
        Ok(TypeErasedBox::new(out))
    }

    #[derive(Debug, PartialEq)]
    struct Bar {
        value: Option<i32>,
    }

    fn deserialize_bar(deser: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
        let mut out = Bar { value: None };
        deser.read_struct(&BAR_SCHEMA, &mut |member, sub| {
            if let Some(0) = member.member_index() {
                out.value = Some(sub.read_integer(member)?);
            }
            Ok(())
        })?;
        Ok(TypeErasedBox::new(out))
    }

    // An alternative deserialize fn for Foo that returns a different type, used to
    // verify `compose` override semantics.
    #[derive(Debug, PartialEq)]
    struct FooReplacement {
        replaced: bool,
    }

    fn deserialize_foo_replacement(
        _deser: &mut dyn ShapeDeserializer,
    ) -> Result<TypeErasedBox, SerdeError> {
        Ok(TypeErasedBox::new(FooReplacement { replaced: true }))
    }

    #[derive(Debug, PartialEq)]
    struct Widget {
        data: Option<Blob>,
    }

    fn deserialize_widget(deser: &mut dyn ShapeDeserializer) -> Result<TypeErasedBox, SerdeError> {
        let mut out = Widget { data: None };
        deser.read_struct(&WIDGET_SCHEMA, &mut |member, sub| {
            if let Some(0) = member.member_index() {
                out.data = Some(sub.read_blob(member)?);
            }
            Ok(())
        })?;
        Ok(TypeErasedBox::new(out))
    }

    // A minimal JSON-like settings impl that base64-decodes strings into
    // blob bytes, the way the JSON codec does on the wire. Used to prove
    // that `deserialize_document` threads protocol settings into the
    // deserializer so blob/timestamp coercion works on the registry path.
    #[derive(Debug)]
    struct JsonishSettings;

    impl DocumentSettings for JsonishSettings {
        fn protocol_id(&self) -> &str {
            "smithy.example#Jsonish"
        }

        fn coerce_string_to_blob(&self, s: &str) -> Result<Vec<u8>, DocumentError> {
            aws_smithy_types::base64::decode(s)
                .map_err(|e| DocumentError::unsupported(format!("invalid base64 blob: {e}")))
        }
    }

    // -- Lookup tests ---------------------------------------------------------------------------

    #[test]
    fn schema_for_returns_registered_schema() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();

        let foo = registry
            .schema_for(&shape_id!("smithy.example", "Foo"))
            .unwrap();
        assert_eq!(foo.shape_id(), FOO_SCHEMA.shape_id());

        let bar = registry
            .schema_for(&shape_id!("smithy.example", "Bar"))
            .unwrap();
        assert_eq!(bar.shape_id(), BAR_SCHEMA.shape_id());
    }

    #[test]
    fn schema_for_returns_none_for_unregistered_shape() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        assert!(registry
            .schema_for(&shape_id!("smithy.example", "DoesNotExist"))
            .is_none());
    }

    #[test]
    fn entry_for_returns_schema_and_fn() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        let entry = registry
            .entry_for(&shape_id!("smithy.example", "Foo"))
            .unwrap();
        assert_eq!(entry.schema().shape_id(), FOO_SCHEMA.shape_id());
        // Function pointer comparison: same fn we registered.
        assert_same_fn(entry.deserialize_fn(), deserialize_foo as DeserializeFn);
    }

    #[test]
    fn entry_for_error_code_resolves_sanitized_codes() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();

        // A bare wire code, a fully qualified code, a URL-suffixed code, and a
        // fully qualified + URL-suffixed code all resolve to the same entry.
        for code in [
            "Foo",
            "smithy.example#Foo",
            "Foo:http://internal.example.com/x",
            "smithy.example#Foo:http://internal.example.com/x",
        ] {
            let entry = registry
                .entry_for_error_code(code)
                .unwrap_or_else(|| panic!("expected a match for {code:?}"));
            assert_eq!(entry.schema().shape_id(), FOO_SCHEMA.shape_id());
        }

        // The scan selects the correct entry among several.
        let bar = registry.entry_for_error_code("Bar").unwrap();
        assert_eq!(bar.schema().shape_id(), BAR_SCHEMA.shape_id());
    }

    #[test]
    fn entry_for_error_code_returns_none_for_unknown_code() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        assert!(registry.entry_for_error_code("DoesNotExist").is_none());
        // A namespace that happens to share the name's text must not match on
        // the namespace component — matching is on the shape name only.
        assert!(registry.entry_for_error_code("Foo#Nope").is_none());
    }

    #[test]
    fn len_and_is_empty() {
        let empty = TypeRegistry::new();
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());

        let populated = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();
        assert_eq!(populated.len(), 2);
        assert!(!populated.is_empty());
    }

    #[test]
    fn iter_yields_all_entries() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();

        let mut ids: Vec<String> = registry.iter().map(|(id, _)| id.to_string()).collect();
        ids.sort();
        assert_eq!(
            ids,
            vec![
                "smithy.example#Bar".to_string(),
                "smithy.example#Foo".to_string(),
            ]
        );
        // ExactSizeIterator: len() agrees with registry len.
        assert_eq!(registry.iter().len(), 2);
    }

    // -- Composition tests ----------------------------------------------------------------------

    #[test]
    fn compose_unions_disjoint_registries() {
        let a = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();
        let b = TypeRegistry::builder()
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();

        let merged = a.compose(b);
        assert_eq!(merged.len(), 2);
        assert!(merged
            .schema_for(&shape_id!("smithy.example", "Foo"))
            .is_some());
        assert!(merged
            .schema_for(&shape_id!("smithy.example", "Bar"))
            .is_some());
    }

    #[test]
    fn compose_lets_other_override_self() {
        // `a` registers `Foo` → deserialize_foo (returns Foo)
        // `b` registers `Foo` → deserialize_foo_replacement (returns FooReplacement)
        // After compose, `Foo` should resolve to the replacement.
        let a = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();
        let b = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo_replacement)
            .build();

        let merged = a.compose(b);
        assert_eq!(merged.len(), 1);

        // Resolve Foo and round-trip through deserialize_document.
        let doc = DiscriminatedDocument::new(Document::Object(Default::default()))
            .with_discriminator(FOO_SCHEMA.shape_id().as_str());
        let boxed = merged.deserialize_document(&doc).unwrap();
        let result = *boxed.downcast::<FooReplacement>().expect("override fn ran");
        assert_eq!(result, FooReplacement { replaced: true });
    }

    #[test]
    fn composed_registry_prefers_primary_then_widens() {
        // `primary` models Foo (deserialize_foo); `fallback` also has Foo but
        // with a different fn, plus Bar. On the Foo collision the primary must
        // win (the opposite precedence of `compose`, where the argument wins),
        // and the lookup must widen to the fallback for Bar.
        let primary = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();
        let fallback = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo_replacement)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();
        let composed = primary.or(&fallback);

        // Collision: primary wins — the entry carries deserialize_foo, not the
        // fallback's replacement fn.
        let foo = composed.entry_for_error_code("Foo").unwrap();
        assert_same_fn(foo.deserialize_fn(), deserialize_foo as DeserializeFn);

        // Widen: Bar resolves from the fallback.
        let bar = composed.entry_for_error_code("Bar").unwrap();
        assert_eq!(bar.schema().shape_id(), BAR_SCHEMA.shape_id());

        // Miss in both: None.
        assert!(composed.entry_for_error_code("Nope").is_none());
    }

    #[test]
    fn composed_registry_entry_for_and_fqn_compose() {
        let primary = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();
        let fallback = TypeRegistry::builder()
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();
        let composed = primary.or(&fallback);

        // entry_for (by ShapeId): primary hit, then fallback widen.
        assert_eq!(
            composed
                .entry_for(&shape_id!("smithy.example", "Foo"))
                .unwrap()
                .schema()
                .shape_id(),
            FOO_SCHEMA.shape_id()
        );
        assert_eq!(
            composed
                .entry_for(&shape_id!("smithy.example", "Bar"))
                .unwrap()
                .schema()
                .shape_id(),
            BAR_SCHEMA.shape_id()
        );

        // entry_for_fqn: same composition, plus a miss in both.
        assert!(composed.entry_for_fqn("smithy.example#Foo").is_some());
        assert!(composed.entry_for_fqn("smithy.example#Bar").is_some());
        assert!(composed.entry_for_fqn("smithy.example#Nope").is_none());
    }

    // Error-constructor stubs for `reify_error` tests. They ignore the
    // deserializer and return a boxed error whose `Display` identifies which
    // registry the entry came from, so the tests can assert scoping/precedence.
    fn error_ctor_primary(
        _deser: &mut dyn ShapeDeserializer,
    ) -> Result<Box<dyn std::error::Error + Send + Sync>, SerdeError> {
        Ok("primary".into())
    }

    fn error_ctor_fallback(
        _deser: &mut dyn ShapeDeserializer,
    ) -> Result<Box<dyn std::error::Error + Send + Sync>, SerdeError> {
        Ok("fallback".into())
    }

    // An error constructor that always fails, used to prove `reify_error`
    // swallows deserialize errors.
    fn error_ctor_always_errors(
        _deser: &mut dyn ShapeDeserializer,
    ) -> Result<Box<dyn std::error::Error + Send + Sync>, SerdeError> {
        Err(SerdeError::custom("boom"))
    }

    #[test]
    fn reify_error_scopes_to_primary_then_widens() {
        // `primary` models Foo (→ "primary"); `fallback` models Bar (→ "fallback").
        // Each entry carries the input-ignoring type-erased stub plus an error
        // constructor, so the test exercises reify_error's plumbing and scoping
        // rather than any specific body shape.
        let primary = TypeRegistry::builder()
            .insert_error_shape(&FOO_SCHEMA, deserialize_foo_replacement, error_ctor_primary)
            .build();
        let fallback = TypeRegistry::builder()
            .insert_error_shape(
                &BAR_SCHEMA,
                deserialize_foo_replacement,
                error_ctor_fallback,
            )
            .build();
        let composed = primary.or(&fallback);
        let doc = Document::Object(Default::default());

        // Operation-scoped hit (primary).
        let boxed = reify_error(composed, "Foo", &mut DocumentShapeDeserializer::new(&doc))
            .expect("Foo reifies from the operation registry");
        assert_eq!(boxed.to_string(), "primary");

        // Widen to the service registry for a code the operation does not model.
        let boxed = reify_error(composed, "Bar", &mut DocumentShapeDeserializer::new(&doc))
            .expect("Bar widens to the service registry");
        assert_eq!(boxed.to_string(), "fallback");

        // Unknown code in both: None.
        assert!(reify_error(composed, "Nope", &mut DocumentShapeDeserializer::new(&doc)).is_none());
    }

    #[test]
    fn reify_error_swallows_deserialize_failure() {
        // The code resolves, but the matched entry's error constructor errors —
        // reify_error must return None rather than propagate, so a malformed
        // unmodeled error can never change how the operation fails.
        let primary = TypeRegistry::builder()
            .insert_error_shape(
                &FOO_SCHEMA,
                deserialize_foo_replacement,
                error_ctor_always_errors,
            )
            .build();
        let empty = TypeRegistry::new();
        let composed = primary.or(&empty);
        let doc = Document::Object(Default::default());

        assert!(reify_error(composed, "Foo", &mut DocumentShapeDeserializer::new(&doc)).is_none());
    }

    // -- deserialize_document tests -------------------------------------------------------------

    #[test]
    fn deserialize_document_round_trip() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();

        // Build a Foo document with a "name" member and the Foo discriminator.
        let mut foo_members = std::collections::HashMap::new();
        foo_members.insert("name".to_string(), Document::String("hello".to_string()));
        let foo_doc = DiscriminatedDocument::new(Document::Object(foo_members))
            .with_discriminator(FOO_SCHEMA.shape_id().as_str());

        let boxed = registry.deserialize_document(&foo_doc).unwrap();
        let foo = *boxed.downcast::<Foo>().expect("downcast to Foo");
        assert_eq!(
            foo,
            Foo {
                name: Some("hello".to_string())
            }
        );

        // Same for Bar.
        let mut bar_members = std::collections::HashMap::new();
        bar_members.insert("value".to_string(), Document::Number(Number::PosInt(42)));
        let bar_doc = DiscriminatedDocument::new(Document::Object(bar_members))
            .with_discriminator(BAR_SCHEMA.shape_id().as_str());

        let boxed = registry.deserialize_document(&bar_doc).unwrap();
        let bar = *boxed.downcast::<Bar>().expect("downcast to Bar");
        assert_eq!(bar, Bar { value: Some(42) });
    }

    #[test]
    fn deserialize_document_errors_when_discriminator_missing() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        // No discriminator attached.
        let doc = DiscriminatedDocument::new(Document::Object(Default::default()));

        let err = registry.deserialize_document(&doc).unwrap_err();
        match err {
            SerdeError::InvalidInput { message } => {
                assert!(
                    message.contains("discriminator"),
                    "expected message to mention discriminator, got: {message}"
                );
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_document_errors_when_shape_id_unregistered() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        let doc = DiscriminatedDocument::new(Document::Object(Default::default()))
            .with_discriminator("smithy.example#Unregistered");

        let err = registry.deserialize_document(&doc).unwrap_err();
        match err {
            SerdeError::UnknownMember { member_name } => {
                assert_eq!(member_name, "smithy.example#Unregistered");
            }
            other => panic!("expected UnknownMember, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_document_propagates_inner_error() {
        // Foo expects `name` to be a String; supplying an integer will trigger a
        // type-mismatch from the deserialize fn.
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        let mut foo_members = std::collections::HashMap::new();
        foo_members.insert("name".to_string(), Document::Number(Number::PosInt(5)));
        let doc = DiscriminatedDocument::new(Document::Object(foo_members))
            .with_discriminator(FOO_SCHEMA.shape_id().as_str());

        let err = registry.deserialize_document(&doc).unwrap_err();
        match err {
            SerdeError::TypeMismatch { .. } => {}
            other => panic!("expected TypeMismatch, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_document_threads_settings_for_blob_coercion() {
        let registry = TypeRegistry::builder()
            .insert_shape(&WIDGET_SCHEMA, deserialize_widget)
            .build();

        // JSON-origin document: the blob arrives as a base64 string and the
        // attached settings know how to decode it. This mirrors what
        // `JsonDeserializer::read_discriminated_document` produces. The fix
        // threads these settings into the deserializer so the nested
        // `read_blob` can coerce.
        let mut members = std::collections::HashMap::new();
        members.insert("data".to_string(), Document::String("YWJjZA==".to_string()));
        let doc = DiscriminatedDocument::new(Document::Object(members))
            .with_discriminator(WIDGET_SCHEMA.shape_id().as_str())
            .with_settings(Arc::new(JsonishSettings));

        let boxed = registry
            .deserialize_document(&doc)
            .expect("blob coercion should succeed when settings are threaded through");
        let widget = *boxed.downcast::<Widget>().expect("downcast to Widget");
        assert_eq!(
            widget,
            Widget {
                data: Some(Blob::new(b"abcd".to_vec())),
            }
        );
    }

    #[test]
    fn deserialize_document_without_settings_uses_base64_default_for_blob() {
        // Same base64-string blob, but no settings attached. `Document`
        // has no blob variant, so a `blob` member always arrives as a
        // string; with no protocol settings the deserializer falls back
        // to the deterministic standard-base64 decode, which is what
        // `DocumentShapeSerializer::write_blob` produces.
        let registry = TypeRegistry::builder()
            .insert_shape(&WIDGET_SCHEMA, deserialize_widget)
            .build();

        let mut members = std::collections::HashMap::new();
        members.insert("data".to_string(), Document::String("YWJjZA==".to_string()));
        let doc = DiscriminatedDocument::new(Document::Object(members))
            .with_discriminator(WIDGET_SCHEMA.shape_id().as_str());

        let widget: Widget = *registry
            .deserialize_document(&doc)
            .expect("base64 default should decode without settings")
            .downcast()
            .expect("registered shape downcasts to Widget");
        assert_eq!(widget.data.unwrap().as_ref(), b"abcd");
    }

    #[test]
    fn deserialize_document_without_settings_rejects_malformed_base64_blob() {
        // The base64 fallback still validates: a string that is not
        // base64 is reported rather than silently accepted.
        let registry = TypeRegistry::builder()
            .insert_shape(&WIDGET_SCHEMA, deserialize_widget)
            .build();

        let mut members = std::collections::HashMap::new();
        members.insert(
            "data".to_string(),
            Document::String("not base64!!!".to_string()),
        );
        let doc = DiscriminatedDocument::new(Document::Object(members))
            .with_discriminator(WIDGET_SCHEMA.shape_id().as_str());

        let err = registry.deserialize_document(&doc).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("base64"), "unexpected error message: {msg}");
    }

    // -- Builder + Debug tests ------------------------------------------------------------------

    #[test]
    fn builder_insert_keyed_form() {
        // Verify that the keyed `insert(id, entry)` form works alongside `insert_shape`.
        let id = FOO_SCHEMA.shape_id();
        let registry = TypeRegistry::builder()
            .insert(id.clone(), RegistryEntry::new(&FOO_SCHEMA, deserialize_foo))
            .build();

        assert_eq!(registry.len(), 1);
        assert!(registry.schema_for(id).is_some());
    }

    #[test]
    fn debug_impls_do_not_panic() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();
        let entry = registry.entry_for(FOO_SCHEMA.shape_id()).unwrap();

        let _ = format!("{registry:?}");
        let _ = format!("{entry:?}");
    }

    // -- Cross-lifetime lookup tests ----------------------------------------------------------

    #[test]
    fn schema_for_works_with_runtime_shape_id() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        // Build a runtime ShapeId<'_> with the same FQN as a registered
        // 'static shape. The registry should resolve it via the
        // FQN-based `Borrow<str>` lookup.
        let owned_fqn = String::from("smithy.example#Foo");
        let owned_ns = String::from("smithy.example");
        let owned_name = String::from("Foo");
        let runtime_id: ShapeId<'_> = ShapeId::from_parts(&owned_fqn, &owned_ns, &owned_name);

        let schema = registry.schema_for(&runtime_id).expect("found via FQN");
        assert_eq!(schema.shape_id(), FOO_SCHEMA.shape_id());

        // Negative case: same lifetime, different FQN.
        let owned_other = String::from("smithy.example#Nope");
        let runtime_miss: ShapeId<'_> = ShapeId::from_parts(&owned_other, &owned_ns, &owned_name);
        assert!(registry.schema_for(&runtime_miss).is_none());
    }

    #[test]
    fn schema_for_fqn_returns_registered_schema() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();

        let foo = registry
            .schema_for_fqn("smithy.example#Foo")
            .expect("registered");
        assert_eq!(foo.shape_id(), FOO_SCHEMA.shape_id());

        assert!(registry.schema_for_fqn("smithy.example#Missing").is_none());
    }

    #[test]
    fn entry_for_fqn_returns_registered_entry() {
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();

        let entry = registry
            .entry_for_fqn("smithy.example#Foo")
            .expect("registered");
        assert_same_fn(entry.deserialize_fn(), deserialize_foo as DeserializeFn);
    }

    // -- Runtime registrations ------------------------------------------------------------------

    const _: () = {
        const fn shared<T: Send + Sync>() {}
        const fn copy<T: Copy>() {}

        shared::<TypeRegistry>();
        shared::<TypeRegistryBuilder>();
        shared::<ResolvedEntry<'static>>();
        shared::<ResolvedIter<'static>>();
        shared::<Arc<dyn RuntimeRegistration>>();
        copy::<ResolvedEntry<'static>>();
    };

    static FOO_MEMBERS: [&Schema<'static>; 1] = [&M_FOO_NAME];
    static BAR_MEMBERS: [&Schema<'static>; 1] = [&M_BAR_VALUE];
    static WIDGET_MEMBERS: [&Schema<'static>; 1] = [&M_WIDGET_DATA];

    // Owned copies of the static test schemas, so runtime registrations hold
    // their schema by value rather than by `'static` reference.
    fn foo_schema() -> Schema<'static> {
        Schema::new_struct(
            shape_id!("smithy.example", "Foo"),
            ShapeType::Structure,
            &FOO_MEMBERS,
        )
    }

    fn bar_schema() -> Schema<'static> {
        Schema::new_struct(
            shape_id!("smithy.example", "Bar"),
            ShapeType::Structure,
            &BAR_MEMBERS,
        )
    }

    fn widget_schema() -> Schema<'static> {
        Schema::new_struct(
            shape_id!("smithy.example", "Widget"),
            ShapeType::Structure,
            &WIDGET_MEMBERS,
        )
    }

    /// What a runtime registration deserializes: the label of the registration
    /// that produced it, and the members it read.
    #[derive(Debug, PartialEq)]
    struct RuntimeValue {
        label: String,
        members: Vec<(String, RuntimeMember)>,
    }

    #[derive(Debug, PartialEq)]
    enum RuntimeMember {
        String(String),
        Integer(i32),
        Blob(Blob),
    }

    #[derive(Clone, Copy)]
    enum ErrorBehavior {
        NotAnError,
        Reify,
        Fail,
    }

    /// A runtime registration that owns its schema and a label, and counts the
    /// calls to `registered_schema`.
    struct OwnedRegistration {
        fqn: String,
        schema: Schema<'static>,
        label: String,
        error: ErrorBehavior,
        schema_calls: Arc<AtomicUsize>,
    }

    fn owned(schema: Schema<'static>, label: &str) -> OwnedRegistration {
        OwnedRegistration {
            fqn: schema.shape_id().as_str().to_string(),
            schema,
            label: label.to_string(),
            error: ErrorBehavior::NotAnError,
            schema_calls: Arc::default(),
        }
    }

    impl OwnedRegistration {
        fn with_error(mut self, error: ErrorBehavior) -> Self {
            self.error = error;
            self
        }

        fn read(
            &self,
            deserializer: &mut dyn ShapeDeserializer,
        ) -> Result<RuntimeValue, SerdeError> {
            let mut members = Vec::new();
            deserializer.read_struct(self.registered_schema(), &mut |member, sub| {
                let value = match member.shape_type() {
                    ShapeType::String => RuntimeMember::String(sub.read_string(member)?),
                    ShapeType::Integer => RuntimeMember::Integer(sub.read_integer(member)?),
                    ShapeType::Blob => RuntimeMember::Blob(sub.read_blob(member)?),
                    other => return Err(SerdeError::custom(format!("unexpected {other:?}"))),
                };
                members.push((member.member_name().unwrap().to_string(), value));
                Ok(())
            })?;
            Ok(RuntimeValue {
                label: self.label.clone(),
                members,
            })
        }
    }

    impl RuntimeRegistration for OwnedRegistration {
        fn registered_shape_id(&self) -> &str {
            &self.fqn
        }

        fn registered_schema(&self) -> &Schema<'_> {
            self.schema_calls.fetch_add(1, Ordering::Relaxed);
            &self.schema
        }

        fn deserialize_registered(
            &self,
            deserializer: &mut dyn ShapeDeserializer,
        ) -> Result<TypeErasedBox, SerdeError> {
            Ok(TypeErasedBox::new(self.read(deserializer)?))
        }

        fn deserialize_registered_error(
            &self,
            deserializer: &mut dyn ShapeDeserializer,
        ) -> Result<Option<Box<dyn std::error::Error + Send + Sync>>, SerdeError> {
            match self.error {
                ErrorBehavior::NotAnError => Ok(None),
                ErrorBehavior::Reify => Ok(Some(self.read(deserializer)?.label.into())),
                ErrorBehavior::Fail => Err(SerdeError::custom("boom")),
            }
        }
    }

    /// Which entry `entry` is: the label of a runtime registration, or for a
    /// static `Foo` entry, which of the two static deserializers it has.
    fn label(entry: ResolvedEntry<'_>) -> String {
        let doc = Document::Object(Default::default());
        let value = entry
            .deserialize(&mut DocumentShapeDeserializer::new(&doc))
            .expect("an empty object deserializes");
        match value.downcast::<RuntimeValue>() {
            Ok(value) => value.label,
            Err(value) if value.downcast_ref::<FooReplacement>().is_some() => {
                "static replacement".to_string()
            }
            Err(_) => "static".to_string(),
        }
    }

    fn runtime_only(registration: OwnedRegistration) -> TypeRegistry {
        TypeRegistry::builder().insert_runtime(registration).build()
    }

    // -- Runtime lookup tests -------------------------------------------------------------------

    #[test]
    fn runtime_entry_resolves_by_fqn_and_shape_id() {
        let registry = runtime_only(owned(foo_schema(), "runtime"));

        let entry = registry.resolve_fqn("smithy.example#Foo").unwrap();
        assert_eq!(entry.shape_id(), "smithy.example#Foo");
        assert_eq!(entry.schema().shape_id().as_str(), "smithy.example#Foo");
        assert_eq!(
            entry.schema().member_schema("name").unwrap().shape_type(),
            ShapeType::String
        );
        assert!(entry.as_runtime().is_some());
        assert!(entry.as_static().is_none());

        // A runtime-built `ShapeId<'_>` resolves through the same FQN.
        let (fqn, ns, name) = (
            String::from("smithy.example#Foo"),
            String::from("smithy.example"),
            String::from("Foo"),
        );
        let runtime_id = ShapeId::from_parts(&fqn, &ns, &name);
        assert_eq!(label(registry.resolve(&runtime_id).unwrap()), "runtime");

        assert!(registry.resolve_fqn("smithy.example#Bar").is_none());
    }

    #[test]
    fn static_lookups_do_not_see_runtime_entries() {
        let registry = runtime_only(owned(foo_schema(), "runtime"));

        assert!(registry.schema_for_fqn("smithy.example#Foo").is_none());
        assert!(registry.schema_for(FOO_SCHEMA.shape_id()).is_none());
        assert!(registry.entry_for_fqn("smithy.example#Foo").is_none());
        assert!(registry.entry_for(FOO_SCHEMA.shape_id()).is_none());
        assert!(registry.entry_for_error_code("Foo").is_none());
        assert_eq!(registry.iter().len(), 0);

        let empty = TypeRegistry::new();
        let composed = registry.or(&empty);
        assert!(composed.entry_for_fqn("smithy.example#Foo").is_none());
        assert!(composed.entry_for_error_code("Foo").is_none());
    }

    #[test]
    fn resolve_covers_static_runtime_and_mixed_registries() {
        let static_only = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .build();
        let runtime_only = runtime_only(owned(foo_schema(), "runtime"));
        let mixed = TypeRegistry::builder()
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .insert_runtime(owned(foo_schema(), "runtime"))
            .build();
        assert!(static_only.runtime.is_none());
        assert!(runtime_only.entries.is_empty());

        assert!(static_only
            .resolve_fqn("smithy.example#Foo")
            .unwrap()
            .as_static()
            .is_some());
        assert!(runtime_only
            .resolve_fqn("smithy.example#Foo")
            .unwrap()
            .as_runtime()
            .is_some());
        assert!(mixed
            .resolve_fqn("smithy.example#Foo")
            .unwrap()
            .as_runtime()
            .is_some());
        assert!(mixed
            .resolve_fqn("smithy.example#Bar")
            .unwrap()
            .as_static()
            .is_some());
        for registry in [&static_only, &runtime_only, &mixed] {
            assert!(registry.resolve_fqn("smithy.example#Nope").is_none());
        }

        // Error codes are sanitized and matched by shape name for both kinds.
        for code in [
            "Foo",
            "smithy.example#Foo",
            "Foo:http://internal.example.com/x",
        ] {
            assert_eq!(
                static_only.resolve_error_code(code).unwrap().shape_id(),
                "smithy.example#Foo"
            );
            assert_eq!(
                runtime_only.resolve_error_code(code).unwrap().shape_id(),
                "smithy.example#Foo"
            );
            assert_eq!(
                mixed.resolve_error_code(code).unwrap().shape_id(),
                "smithy.example#Foo"
            );
        }
        assert_eq!(
            mixed.resolve_error_code("Bar").unwrap().shape_id(),
            "smithy.example#Bar"
        );
        for registry in [&static_only, &runtime_only, &mixed] {
            assert!(registry.resolve_error_code("Nope").is_none());
            assert!(registry.resolve_error_code("Foo#Nope").is_none());
        }
    }

    #[test]
    fn resolved_static_entry_reports_its_registered_key() {
        // The explicit `insert(id, entry)` form may key an entry by an ID other
        // than its schema's; the resolved entry reports the key.
        let registry = TypeRegistry::builder()
            .insert(
                shape_id!("smithy.example", "Alias"),
                RegistryEntry::new(&FOO_SCHEMA, deserialize_foo),
            )
            .build();

        let entry = registry.resolve_fqn("smithy.example#Alias").unwrap();
        assert_eq!(entry.shape_id(), "smithy.example#Alias");
        assert_eq!(entry.schema().shape_id().as_str(), "smithy.example#Foo");
        assert_eq!(
            registry.resolved_entries().next().unwrap().0,
            "smithy.example#Alias"
        );
    }

    #[test]
    fn shape_name_of_matches_shape_id_shape_name() {
        assert_eq!(shape_name_of("smithy.example#Foo"), "Foo");
        assert_eq!(shape_name_of("smithy.example#Foo$name"), "Foo");
        assert_eq!(shape_name_of("Foo"), "Foo");
        assert_eq!(
            shape_name_of(M_FOO_NAME.shape_id().as_str()),
            M_FOO_NAME.shape_id().shape_name()
        );
    }

    // -- Runtime insertion and composition tests ------------------------------------------------

    #[test]
    fn insertion_replaces_entries_of_either_kind() {
        // Runtime over static.
        let registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_runtime(owned(foo_schema(), "runtime"))
            .build();
        assert_eq!(registry.len(), 1);
        assert!(registry.entries.is_empty());
        assert_eq!(
            label(registry.resolve_fqn("smithy.example#Foo").unwrap()),
            "runtime"
        );

        // Static over runtime, by each static insertion method. The emptied
        // runtime map is freed.
        let builders = [
            TypeRegistry::builder()
                .insert_runtime(owned(foo_schema(), "runtime"))
                .insert_shape(&FOO_SCHEMA, deserialize_foo),
            TypeRegistry::builder()
                .insert_runtime(owned(foo_schema(), "runtime"))
                .insert_error_shape(&FOO_SCHEMA, deserialize_foo, error_ctor_primary),
            TypeRegistry::builder()
                .insert_runtime(owned(foo_schema(), "runtime"))
                .insert(
                    FOO_SCHEMA.shape_id().clone(),
                    RegistryEntry::new(&FOO_SCHEMA, deserialize_foo),
                ),
        ];
        for builder in builders {
            let registry = builder.build();
            assert_eq!(registry.len(), 1);
            assert!(registry.runtime.is_none());
            assert_eq!(
                label(registry.resolve_fqn("smithy.example#Foo").unwrap()),
                "static"
            );
        }

        // Runtime over runtime.
        let registry = TypeRegistry::builder()
            .insert_runtime(owned(foo_schema(), "first"))
            .insert_runtime(owned(foo_schema(), "second"))
            .build();
        assert_eq!(registry.len(), 1);
        assert_eq!(
            label(registry.resolve_fqn("smithy.example#Foo").unwrap()),
            "second"
        );
    }

    #[test]
    fn compose_lets_other_win_across_kinds() {
        #[derive(Clone, Copy, Debug)]
        enum Kind {
            Static,
            Runtime,
        }

        fn foo_registry(kind: Kind, is_other: bool) -> TypeRegistry {
            match (kind, is_other) {
                (Kind::Static, false) => TypeRegistry::builder()
                    .insert_shape(&FOO_SCHEMA, deserialize_foo)
                    .build(),
                (Kind::Static, true) => TypeRegistry::builder()
                    .insert_shape(&FOO_SCHEMA, deserialize_foo_replacement)
                    .build(),
                (Kind::Runtime, false) => runtime_only(owned(foo_schema(), "runtime self")),
                (Kind::Runtime, true) => runtime_only(owned(foo_schema(), "runtime other")),
            }
        }

        for (own, other, expected) in [
            (Kind::Static, Kind::Static, "static replacement"),
            (Kind::Static, Kind::Runtime, "runtime other"),
            (Kind::Runtime, Kind::Static, "static replacement"),
            (Kind::Runtime, Kind::Runtime, "runtime other"),
        ] {
            let composed = foo_registry(own, false).compose(foo_registry(other, true));
            assert_eq!(composed.len(), 1, "{own:?} composed with {other:?}");
            assert_eq!(
                label(composed.resolve_fqn("smithy.example#Foo").unwrap()),
                expected,
                "{own:?} composed with {other:?}"
            );
            // A registry left without runtime entries frees its runtime map.
            assert_eq!(
                composed.runtime.is_some(),
                matches!(other, Kind::Runtime),
                "{own:?} composed with {other:?}"
            );
        }

        // Disjoint entries of both kinds are all kept.
        let composed = runtime_only(owned(foo_schema(), "runtime")).compose(
            TypeRegistry::builder()
                .insert_shape(&BAR_SCHEMA, deserialize_bar)
                .insert_runtime(owned(widget_schema(), "widget"))
                .build(),
        );
        assert_eq!(composed.len(), 3);
        assert_eq!(composed.iter().len(), 1);
        assert_eq!(
            label(composed.resolve_fqn("smithy.example#Foo").unwrap()),
            "runtime"
        );
        assert_eq!(
            label(composed.resolve_fqn("smithy.example#Widget").unwrap()),
            "widget"
        );
    }

    #[test]
    fn composed_registry_resolve_prefers_primary_across_kinds() {
        let static_foo = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .build();
        let runtime_foo = TypeRegistry::builder()
            .insert_runtime(owned(foo_schema(), "runtime"))
            .insert_runtime(owned(bar_schema(), "runtime bar"))
            .build();
        let primary_only_foo = |registry: &TypeRegistry| -> TypeRegistry {
            match registry
                .resolve_fqn("smithy.example#Foo")
                .unwrap()
                .as_static()
            {
                Some(_) => TypeRegistry::builder()
                    .insert_shape(&FOO_SCHEMA, deserialize_foo_replacement)
                    .build(),
                None => runtime_only(owned(foo_schema(), "primary runtime")),
            }
        };

        // Each primary has only Foo, of the same kind as its fallback or not.
        for fallback in [&static_foo, &runtime_foo] {
            for primary in [
                primary_only_foo(&static_foo),
                primary_only_foo(&runtime_foo),
            ] {
                let expected = label(primary.resolve_fqn("smithy.example#Foo").unwrap());
                let composed = primary.or(fallback);

                assert_eq!(
                    label(composed.resolve_fqn("smithy.example#Foo").unwrap()),
                    expected
                );
                assert_eq!(
                    label(composed.resolve(FOO_SCHEMA.shape_id()).unwrap()),
                    expected
                );
                assert_eq!(label(composed.resolve_error_code("Foo").unwrap()), expected);

                // Widen to the fallback for Bar.
                assert_eq!(
                    composed
                        .resolve_fqn("smithy.example#Bar")
                        .unwrap()
                        .shape_id(),
                    "smithy.example#Bar"
                );
                assert_eq!(
                    composed.resolve_error_code("Bar").unwrap().shape_id(),
                    "smithy.example#Bar"
                );
                assert!(composed.resolve_fqn("smithy.example#Nope").is_none());
                assert!(composed.resolve_error_code("Nope").is_none());
            }
        }

        // A resolved entry borrows the layered registries, not the temporary view.
        let entry = runtime_foo
            .or(&static_foo)
            .resolve_fqn("smithy.example#Foo");
        assert_eq!(label(entry.unwrap()), "runtime");
    }

    #[test]
    fn shared_registration_is_shared_between_registries() {
        let registration: Arc<dyn RuntimeRegistration> =
            Arc::new(owned(foo_schema(), "shared").with_error(ErrorBehavior::Reify));
        let service = TypeRegistry::builder()
            .insert_shared_runtime(registration.clone())
            .build();
        let operation = TypeRegistry::builder()
            .extend_shared_runtime([
                registration.clone(),
                Arc::new(owned(bar_schema(), "bar")) as Arc<dyn RuntimeRegistration>,
            ])
            .build();
        assert_eq!(Arc::strong_count(&registration), 3);
        assert_eq!(operation.len(), 2);
        assert_eq!(
            label(service.resolve_fqn("smithy.example#Foo").unwrap()),
            "shared"
        );
        assert_eq!(
            label(operation.resolve_fqn("smithy.example#Foo").unwrap()),
            "shared"
        );

        drop((service, operation));
        assert_eq!(Arc::strong_count(&registration), 1);
    }

    #[test]
    fn extend_without_registrations_leaves_no_runtime_map() {
        /// An iterator whose size hint promises more items than it yields.
        struct Overpromising;

        impl Iterator for Overpromising {
            type Item = Arc<dyn RuntimeRegistration>;

            fn next(&mut self) -> Option<Self::Item> {
                None
            }

            fn size_hint(&self) -> (usize, Option<usize>) {
                (4, None)
            }
        }

        let empty = TypeRegistry::builder()
            .extend_shared_runtime(Vec::new())
            .build();
        assert!(empty.runtime.is_none());

        let overpromised = TypeRegistry::builder()
            .extend_shared_runtime(Overpromising)
            .build();
        assert!(overpromised.runtime.is_none());
        assert!(overpromised.is_empty());
    }

    // -- Runtime iteration and size tests -------------------------------------------------------

    #[test]
    fn resolved_entries_include_both_kinds_and_iter_only_static() {
        let registry = TypeRegistry::builder()
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .insert_runtime(owned(foo_schema(), "runtime"))
            .build();

        assert_eq!(registry.len(), 2);
        assert!(!registry.is_empty());
        assert_eq!(registry.iter().len(), 1);
        assert_eq!(
            registry.iter().next().unwrap().0.as_str(),
            "smithy.example#Bar"
        );

        let mut resolved = registry.resolved_entries();
        assert_eq!(resolved.len(), registry.len());
        // Static entries come first.
        let (fqn, entry) = resolved.next().unwrap();
        assert_eq!(
            (fqn, entry.as_static().is_some()),
            ("smithy.example#Bar", true)
        );
        assert_eq!(resolved.len(), 1);
        let (fqn, entry) = resolved.next().unwrap();
        assert_eq!(
            (fqn, entry.as_runtime().is_some()),
            ("smithy.example#Foo", true)
        );
        assert_eq!(fqn, entry.shape_id());
        assert_eq!(resolved.len(), 0);
        assert!(resolved.next().is_none());
    }

    #[test]
    fn len_and_is_empty_count_runtime_entries() {
        let registry = runtime_only(owned(foo_schema(), "runtime"));
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
        assert_eq!(registry.iter().len(), 0);
        assert_eq!(registry.resolved_entries().len(), 1);
    }

    // -- Runtime deserialization tests ----------------------------------------------------------

    #[test]
    fn deserialize_document_uses_runtime_entries() {
        let registry = TypeRegistry::builder()
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .insert_runtime(owned(foo_schema(), "runtime"))
            .build();

        let mut members = std::collections::HashMap::new();
        members.insert("name".to_string(), Document::String("hello".to_string()));
        let doc = DiscriminatedDocument::new(Document::Object(members))
            .with_discriminator("smithy.example#Foo");
        let value = *registry
            .deserialize_document(&doc)
            .unwrap()
            .downcast::<RuntimeValue>()
            .expect("the runtime registration's value");
        assert_eq!(
            value,
            RuntimeValue {
                label: "runtime".to_string(),
                members: vec![(
                    "name".to_string(),
                    RuntimeMember::String("hello".to_string())
                )],
            }
        );

        // The static entry in the same registry still deserializes.
        let mut members = std::collections::HashMap::new();
        members.insert("value".to_string(), Document::Number(Number::PosInt(42)));
        let doc = DiscriminatedDocument::new(Document::Object(members))
            .with_discriminator("smithy.example#Bar");
        let bar = *registry
            .deserialize_document(&doc)
            .unwrap()
            .downcast::<Bar>()
            .unwrap();
        assert_eq!(bar, Bar { value: Some(42) });

        let doc = DiscriminatedDocument::new(Document::Object(Default::default()))
            .with_discriminator("smithy.example#Unregistered");
        match registry.deserialize_document(&doc).unwrap_err() {
            SerdeError::UnknownMember { member_name } => {
                assert_eq!(member_name, "smithy.example#Unregistered");
            }
            other => panic!("expected UnknownMember, got {other:?}"),
        }
    }

    #[test]
    fn deserialize_document_threads_settings_to_runtime_entries() {
        let registry = runtime_only(owned(widget_schema(), "runtime"));

        let mut members = std::collections::HashMap::new();
        members.insert("data".to_string(), Document::String("YWJjZA==".to_string()));
        let doc = DiscriminatedDocument::new(Document::Object(members))
            .with_discriminator("smithy.example#Widget")
            .with_settings(Arc::new(JsonishSettings));
        let value = *registry
            .deserialize_document(&doc)
            .unwrap()
            .downcast::<RuntimeValue>()
            .unwrap();
        assert_eq!(
            value.members,
            vec![(
                "data".to_string(),
                RuntimeMember::Blob(Blob::new(b"abcd".to_vec()))
            )]
        );
    }

    #[test]
    fn reify_error_uses_runtime_entries() {
        let runtime_operation = TypeRegistry::builder()
            .insert_runtime(owned(foo_schema(), "operation").with_error(ErrorBehavior::Reify))
            .build();
        let static_service = TypeRegistry::builder()
            .insert_error_shape(
                &FOO_SCHEMA,
                deserialize_foo_replacement,
                error_ctor_fallback,
            )
            .insert_error_shape(
                &BAR_SCHEMA,
                deserialize_foo_replacement,
                error_ctor_fallback,
            )
            .build();
        let doc = Document::Object(Default::default());
        let reify = |registry: ComposedRegistry<'_>, code: &str| {
            reify_error(registry, code, &mut DocumentShapeDeserializer::new(&doc))
                .map(|error| error.to_string())
        };

        // The runtime operation error shadows the static service error, and Bar
        // widens to the static service registry.
        let composed = runtime_operation.or(&static_service);
        assert_eq!(reify(composed, "Foo").as_deref(), Some("operation"));
        assert_eq!(reify(composed, "Bar").as_deref(), Some("fallback"));
        assert_eq!(reify(composed, "Nope"), None);

        // A runtime service registry under a static operation registry.
        let static_operation = TypeRegistry::builder()
            .insert_error_shape(&BAR_SCHEMA, deserialize_foo_replacement, error_ctor_primary)
            .build();
        let composed = static_operation.or(&runtime_operation);
        assert_eq!(reify(composed, "Bar").as_deref(), Some("primary"));
        assert_eq!(reify(composed, "Foo").as_deref(), Some("operation"));

        // A runtime entry that is not an error, or whose error fails to
        // deserialize, reifies nothing.
        let empty = TypeRegistry::new();
        for behavior in [ErrorBehavior::NotAnError, ErrorBehavior::Fail] {
            let registry = runtime_only(owned(foo_schema(), "runtime").with_error(behavior));
            assert_eq!(reify(registry.or(&empty), "Foo"), None);
        }
    }

    #[test]
    fn resolved_entry_deserialize_error_distinguishes_non_errors_from_failures() {
        let doc = Document::Object(Default::default());
        let static_registry = TypeRegistry::builder()
            .insert_shape(&FOO_SCHEMA, deserialize_foo)
            .insert_error_shape(&BAR_SCHEMA, deserialize_bar, error_ctor_always_errors)
            .build();
        let deserialize_error = |entry: ResolvedEntry<'_>| {
            entry
                .deserialize_error(&mut DocumentShapeDeserializer::new(&doc))
                .map(|error| error.map(|error| error.to_string()))
        };

        let foo = static_registry.resolve_fqn("smithy.example#Foo").unwrap();
        assert!(matches!(deserialize_error(foo), Ok(None)));
        let bar = static_registry.resolve_fqn("smithy.example#Bar").unwrap();
        assert!(deserialize_error(bar).is_err());

        for (behavior, expected) in [
            (ErrorBehavior::NotAnError, Ok(None)),
            (ErrorBehavior::Reify, Ok(Some("runtime".to_string()))),
            (ErrorBehavior::Fail, Err(())),
        ] {
            let registry = runtime_only(owned(foo_schema(), "runtime").with_error(behavior));
            let entry = registry.resolve_fqn("smithy.example#Foo").unwrap();
            assert_eq!(deserialize_error(entry).map_err(|_| ()), expected);
        }
    }

    // -- Laziness and contract tests ------------------------------------------------------------

    #[test]
    fn runtime_schemas_are_not_requested_until_used() {
        let foo = owned(foo_schema(), "foo").with_error(ErrorBehavior::Reify);
        let bar = owned(bar_schema(), "bar");
        let foo_calls = foo.schema_calls.clone();
        let bar_calls = bar.schema_calls.clone();
        let schema_calls = || foo_calls.load(Ordering::Relaxed) + bar_calls.load(Ordering::Relaxed);

        let builder = TypeRegistry::builder()
            .insert_runtime(foo)
            .extend_shared_runtime([Arc::new(bar) as Arc<dyn RuntimeRegistration>]);
        let _ = format!("{builder:?}");
        let registry = builder.build();
        let static_registry = TypeRegistry::builder()
            .insert_shape(&WIDGET_SCHEMA, deserialize_widget)
            .build();
        let composed = registry.or(&static_registry);

        // Lookups, iteration, sizes, and formatting use only registered shape IDs.
        let entry = registry.resolve_fqn("smithy.example#Foo").unwrap();
        assert_eq!(entry.shape_id(), "smithy.example#Foo");
        assert!(registry.resolve(BAR_SCHEMA.shape_id()).is_some());
        assert!(registry.resolve_error_code("Bar").is_some());
        assert!(composed.resolve_fqn("smithy.example#Widget").is_some());
        assert!(composed.resolve_error_code("Foo").is_some());
        let mut fqns: Vec<_> = registry
            .resolved_entries()
            .map(|(_, entry)| entry.shape_id())
            .collect();
        fqns.sort();
        assert_eq!(fqns, ["smithy.example#Bar", "smithy.example#Foo"]);
        assert_eq!(registry.len(), 2);
        let _ = format!("{registry:?} {entry:?}");
        let registry = registry.compose(TypeRegistry::new());
        assert_eq!(schema_calls(), 0);

        // Asking for the schema, or deserializing, uses it.
        let entry = registry.resolve_fqn("smithy.example#Foo").unwrap();
        entry.schema();
        assert_eq!(schema_calls(), 1);
        let doc = Document::Object(Default::default());
        entry
            .deserialize(&mut DocumentShapeDeserializer::new(&doc))
            .unwrap();
        assert_eq!(schema_calls(), 2);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "must have its registered_shape_id")]
    fn debug_builds_reject_a_schema_that_differs_from_its_registered_shape_id() {
        let mut registration = owned(foo_schema(), "runtime");
        registration.fqn = "smithy.example#Other".to_string();
        let registry = runtime_only(registration);

        registry
            .resolve_fqn("smithy.example#Other")
            .expect("indexed by the registered shape ID")
            .schema();
    }

    #[test]
    fn debug_reports_counts_and_kinds() {
        let registry = TypeRegistry::builder()
            .insert_shape(&BAR_SCHEMA, deserialize_bar)
            .insert_runtime(owned(foo_schema(), "runtime"))
            .build();

        assert_eq!(
            format!("{registry:?}"),
            "TypeRegistry { entries: 2, static_entries: 1, runtime_entries: 1, .. }"
        );
        assert_eq!(
            format!("{:?}", registry.resolve_fqn("smithy.example#Foo").unwrap()),
            r#"ResolvedEntry { shape_id: "smithy.example#Foo", kind: "runtime" }"#
        );
        assert_eq!(
            format!("{:?}", registry.resolve_fqn("smithy.example#Bar").unwrap()),
            r#"ResolvedEntry { shape_id: "smithy.example#Bar", kind: "static" }"#
        );
    }
}
