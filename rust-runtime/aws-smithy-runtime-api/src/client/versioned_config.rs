/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Version-stable [`ConfigBag`](aws_smithy_types::config_bag::ConfigBag) entries for values
//! defined by independently versioned crates.
//!
//! # The problem
//!
//! A `ConfigBag` is keyed by [`TypeId`](std::any::TypeId). Two source-identical types from semver-incompatible
//! releases of a crate (for example `aws-smithy-schema` 1.x and 2.x, or `aws-smithy-eventstream`
//! 0.60 and 0.61) have different `TypeId`s, so a value stored through one release is silently
//! invisible to code compiled against the other: the lookup returns `None`, which is
//! indistinguishable from "not configured".
//!
//! # The solution
//!
//! Each cross-version configuration concept gets a *logical slot*: a marker type declared in this
//! crate. A slot-specific public wrapper is the `ConfigBag` entry. Because the slot marker and the
//! wrapper both belong to this stable crate, the entry's `TypeId` does not change when the crate
//! that defines the payload takes an incompatible release.
//!
//! The payload itself is type-erased. It is a type from the independently versioned crate,
//! associated with the slot through [`ConfigPayloadFor`], and it records which crate and
//! compatibility line produced it as a [`RepresentationId`]. Slot-specific wrappers safely
//! downcast to each representation a consumer supports and report anything else with a
//! [`ConfigSlotError`] instead of treating it as absent. The generic storage implementation stays
//! private so callers cannot bypass those wrappers.
//!
//! Because every representation shares one key, `ConfigBag` replacement, append, unset, clear, and
//! layer precedence apply to the logical setting: a schema 2.x value in a higher layer replaces a
//! schema 1.x value in a lower one, and an explicit unset suppresses both.
//!
//! Slots can only be declared in this crate. A slot declared in the payload crate would acquire
//! that crate's identity and reintroduce the original problem, so slot declarations are sealed.

use std::any::Any;
use std::error::Error as StdError;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

/// Identifies the crate and compatibility line that produced a versioned configuration payload.
///
/// A *compatibility line* follows Cargo's semver compatibility rules: `1` for any 1.x release, `2`
/// for any 2.x release, and `0.61` for any 0.61.x release. Within one compatibility line, an
/// `api_revision` distinguishes parallel representations, such as a second version of a trait that
/// a crate introduces in a minor release.
///
/// Representation IDs drive selection, diagnostics and compatibility checks. They are not a
/// substitute for type safety: consumers must downcast before using a payload.
///
/// Use the [`representation_id!`](crate::representation_id) macro to build one for the current
/// crate from its Cargo metadata.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RepresentationId {
    package: &'static str,
    compatibility_line: &'static str,
    api_revision: u32,
}

impl RepresentationId {
    /// Creates a representation ID.
    ///
    /// Prefer [`representation_id!`](crate::representation_id), which derives `package` and
    /// `compatibility_line` from the calling crate's Cargo metadata so they cannot drift from the
    /// crate's real version.
    pub const fn new(
        package: &'static str,
        compatibility_line: &'static str,
        api_revision: u32,
    ) -> Self {
        Self {
            package,
            compatibility_line,
            api_revision,
        }
    }

    /// Returns the Cargo compatibility line for a crate version.
    ///
    /// `major` is the version's major component and `major_minor` is `"{major}.{minor}"`. Returns
    /// `major_minor` for a 0.x version and `major` otherwise, matching Cargo's rule that 0.x minor
    /// releases are mutually incompatible.
    pub const fn compatibility_line_for(
        major: &'static str,
        major_minor: &'static str,
    ) -> &'static str {
        let bytes = major.as_bytes();
        if bytes.len() == 1 && bytes[0] == b'0' {
            major_minor
        } else {
            major
        }
    }

    /// Returns the name of the crate that produced the payload, for example `aws-smithy-schema`.
    pub const fn package(&self) -> &'static str {
        self.package
    }

    /// Returns the compatibility line of the crate that produced the payload, for example `1` or
    /// `0.61`.
    pub const fn compatibility_line(&self) -> &'static str {
        self.compatibility_line
    }

    /// Returns the API revision of the payload within its compatibility line.
    pub const fn api_revision(&self) -> u32 {
        self.api_revision
    }
}

impl fmt::Display for RepresentationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}@{} (api revision {})",
            self.package, self.compatibility_line, self.api_revision
        )
    }
}

/// Builds a [`RepresentationId`] for the calling crate.
///
/// The package name and compatibility line come from the calling crate's Cargo metadata
/// (`CARGO_PKG_NAME`, `CARGO_PKG_VERSION_MAJOR` and `CARGO_PKG_VERSION_MINOR`), so they always
/// match the crate's real version. The single argument is the API revision.
///
/// ```
/// use aws_smithy_runtime_api::client::versioned_config::RepresentationId;
///
/// const REPRESENTATION: RepresentationId = aws_smithy_runtime_api::representation_id!(1);
/// assert_eq!("aws-smithy-runtime-api", REPRESENTATION.package());
/// assert_eq!(1, REPRESENTATION.api_revision());
/// ```
#[macro_export]
macro_rules! representation_id {
    ($api_revision:expr) => {
        $crate::client::versioned_config::RepresentationId::new(
            ::core::env!("CARGO_PKG_NAME"),
            $crate::client::versioned_config::RepresentationId::compatibility_line_for(
                ::core::env!("CARGO_PKG_VERSION_MAJOR"),
                ::core::concat!(
                    ::core::env!("CARGO_PKG_VERSION_MAJOR"),
                    ".",
                    ::core::env!("CARGO_PKG_VERSION_MINOR")
                ),
            ),
            $api_revision,
        )
    };
}

pub(crate) mod private {
    pub(crate) trait Sealed {}
}

/// Internal identity and diagnostic metadata for a logical configuration slot.
pub(crate) trait ConfigSlot: private::Sealed + Send + Sync + 'static {
    /// A human-readable name for the setting, used in diagnostics, for example `client protocol`.
    const NAME: &'static str;
}

/// Associates a payload type from an independently versioned crate with a stable logical slot.
///
/// The payload crate implements this for its own type; Rust's coherence rules allow that because
/// the payload type is local to it. Each compatibility line of the payload crate implements it for
/// its own payload type, with its own [`REPRESENTATION`](Self::REPRESENTATION).
///
/// Do not also implement `Storable` for the payload type. The stable slot-specific wrapper is the
/// only bag entry for the logical setting.
pub trait ConfigPayloadFor<Slot>: Any + fmt::Debug + Send + Sync + 'static {
    /// Identifies the crate, compatibility line and API revision that define this payload type.
    ///
    /// Build it with [`representation_id!`](crate::representation_id).
    const REPRESENTATION: RepresentationId;
}

trait Payload: Any + fmt::Debug + Send + Sync {}
impl<T: Any + fmt::Debug + Send + Sync> Payload for T {}

/// Internal type-erased storage for a logical configuration slot.
///
/// Holds a type-erased payload plus the [`RepresentationId`] of the crate that produced it. The
/// payload is shared through an [`Arc`], so cloning the wrapper is cheap and does not require the
/// payload to be `Clone`; this also makes the wrapper usable in a
/// [`CloneableLayer`](aws_smithy_types::config_bag::CloneableLayer).
///
/// See the [module documentation](self) for why this type exists.
pub(crate) struct VersionedConfigValue<S: ConfigSlot> {
    representation: RepresentationId,
    payload: Arc<dyn Payload>,
    _slot: PhantomData<fn() -> S>,
}

impl<S: ConfigSlot> VersionedConfigValue<S> {
    /// Wraps a payload that is already shared.
    ///
    /// Useful when the same allocation must also be held through a slot-specific capability
    /// trait object.
    pub(crate) fn from_arc<T: ConfigPayloadFor<S>>(payload: Arc<T>) -> Self {
        Self {
            representation: T::REPRESENTATION,
            payload,
            _slot: PhantomData,
        }
    }

    /// Identifies the crate and compatibility line that produced the payload.
    pub(crate) fn representation(&self) -> RepresentationId {
        self.representation
    }

    /// Returns the payload if it is a `T`.
    ///
    /// `None` means the payload is a different representation, typically from a different
    /// compatibility line of the payload crate. Consumers that support several representations try
    /// each in turn and report the remaining case with
    /// [`unsupported_error`](Self::unsupported_error); they must not treat it as an absent value.
    pub(crate) fn downcast_ref<T: ConfigPayloadFor<S>>(&self) -> Option<&T> {
        self.payload_any().downcast_ref::<T>()
    }

    /// Returns a shared handle to the payload if it is a `T`.
    pub(crate) fn downcast_arc<T: ConfigPayloadFor<S>>(&self) -> Option<Arc<T>> {
        let payload: Arc<dyn Any + Send + Sync> = self.payload.clone();
        payload.downcast::<T>().ok()
    }

    /// Builds the error for a payload that none of the consumer's `supported` representations
    /// could downcast.
    ///
    /// If the payload's representation is in `supported`, the payload claims a representation the
    /// consumer understands but is not the type the consumer was compiled against. That usually
    /// means two copies of the payload crate with the same compatibility line are linked, and is
    /// reported separately from a representation the consumer does not support at all.
    pub(crate) fn unsupported_error(
        &self,
        supported: &'static [RepresentationId],
    ) -> ConfigSlotError {
        let kind = if supported.contains(&self.representation) {
            ConfigSlotErrorKind::RepresentationMismatch {
                found: self.representation,
            }
        } else {
            ConfigSlotErrorKind::Unsupported {
                found: self.representation,
                supported,
            }
        };
        ConfigSlotError {
            slot: S::NAME,
            kind,
        }
    }

    fn payload_any(&self) -> &dyn Any {
        &*self.payload
    }
}

impl<S: ConfigSlot> Clone for VersionedConfigValue<S> {
    fn clone(&self) -> Self {
        Self {
            representation: self.representation,
            payload: self.payload.clone(),
            _slot: PhantomData,
        }
    }
}

impl<S: ConfigSlot> fmt::Debug for VersionedConfigValue<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VersionedConfigValue")
            .field("slot", &S::NAME)
            .field("representation", &self.representation)
            .field("payload", &self.payload)
            .finish()
    }
}

/// Declares a [`ConfigSlot`] marker type.
///
/// Crate-private so that every slot is declared in this stable crate. `storage` selects how the
/// slot's value is stored:
///
/// - `replace`: `VersionedConfigValue<Marker>` is `Storable` with `StoreReplace`.
/// - `append`: `VersionedConfigValue<Marker>` is `Storable` with `StoreAppend`.
/// - `wrapper`: no `Storable` impl is generated, because a slot-specific wrapper around
///   `VersionedConfigValue<Marker>` is the bag entry instead. Exactly one type must be `Storable`
///   for a slot, or the slot would occupy two keys.
macro_rules! config_slot {
    (
        $(#[$meta:meta])*
        $vis:vis enum $marker:ident { name: $name:literal, storage: $storage:ident $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Debug)]
        $vis enum $marker {}

        impl $crate::client::versioned_config::private::Sealed for $marker {}

        impl $crate::client::versioned_config::ConfigSlot for $marker {
            const NAME: &'static str = $name;
        }

        $crate::client::versioned_config::config_slot!(@storage $storage $marker);
    };
    (@storage replace $marker:ident) => {
        impl ::aws_smithy_types::config_bag::Storable
            for $crate::client::versioned_config::VersionedConfigValue<$marker>
        {
            type Storer = ::aws_smithy_types::config_bag::StoreReplace<Self>;
        }
    };
    (@storage append $marker:ident) => {
        impl ::aws_smithy_types::config_bag::Storable
            for $crate::client::versioned_config::VersionedConfigValue<$marker>
        {
            type Storer = ::aws_smithy_types::config_bag::StoreAppend<Self>;
        }
    };
    (@storage wrapper $marker:ident) => {};
}
pub(crate) use config_slot;

/// A versioned configuration value is missing or cannot be used by this consumer.
#[derive(Debug)]
pub struct ConfigSlotError {
    slot: &'static str,
    kind: ConfigSlotErrorKind,
}

#[derive(Debug)]
enum ConfigSlotErrorKind {
    Missing,
    Unsupported {
        found: RepresentationId,
        supported: &'static [RepresentationId],
    },
    RepresentationMismatch {
        found: RepresentationId,
    },
}

impl ConfigSlotError {
    /// Creates the error for a slot with no configured value.
    pub(crate) fn missing<S: ConfigSlot>() -> Self {
        Self {
            slot: S::NAME,
            kind: ConfigSlotErrorKind::Missing,
        }
    }

    /// Returns the name of the slot, for example `client protocol`.
    pub fn slot_name(&self) -> &'static str {
        self.slot
    }

    /// Returns the configured value's representation, or `None` if no value was configured.
    pub fn found(&self) -> Option<RepresentationId> {
        match self.kind {
            ConfigSlotErrorKind::Missing => None,
            ConfigSlotErrorKind::Unsupported { found, .. }
            | ConfigSlotErrorKind::RepresentationMismatch { found } => Some(found),
        }
    }

    /// Returns `true` if no value was configured.
    pub fn is_missing(&self) -> bool {
        matches!(self.kind, ConfigSlotErrorKind::Missing)
    }

    /// Returns `true` if the configured value's representation is not one this consumer supports.
    pub fn is_unsupported(&self) -> bool {
        matches!(self.kind, ConfigSlotErrorKind::Unsupported { .. })
    }

    /// Returns `true` if the configured value claims a supported representation but is not the
    /// type this consumer was compiled against.
    pub fn is_representation_mismatch(&self) -> bool {
        matches!(
            self.kind,
            ConfigSlotErrorKind::RepresentationMismatch { .. }
        )
    }
}

impl fmt::Display for ConfigSlotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ConfigSlotErrorKind::Missing => write!(f, "no {} is configured", self.slot),
            ConfigSlotErrorKind::Unsupported { found, supported } => {
                write!(
                    f,
                    "the configured {} was built with {found}, but this client supports [",
                    self.slot
                )?;
                for (i, representation) in supported.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{representation}")?;
                }
                f.write_str("]")
            }
            ConfigSlotErrorKind::RepresentationMismatch { found } => write!(
                f,
                "the configured {} claims to be built with {found}, which this client supports, \
                 but it is not the type this client was compiled against; this usually means two \
                 copies of {} are linked",
                self.slot,
                found.package()
            ),
        }
    }
}

impl StdError for ConfigSlotError {}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_types::config_bag::{CloneableLayer, ConfigBag, Layer};

    config_slot! {
        /// A test slot stored with `StoreReplace`.
        enum TestSlot { name: "test setting", storage: replace }
    }

    config_slot! {
        /// A test slot stored with `StoreAppend`.
        enum TestListSlot { name: "test list", storage: append }
    }

    /// Stands in for the payload crate's 1.x release.
    mod v1 {
        use super::*;

        #[derive(Debug)]
        pub(super) struct Setting(pub(super) &'static str);

        pub(super) const REPRESENTATION: RepresentationId =
            RepresentationId::new("test-payloads", "1", 1);

        impl ConfigPayloadFor<TestSlot> for Setting {
            const REPRESENTATION: RepresentationId = REPRESENTATION;
        }

        impl ConfigPayloadFor<TestListSlot> for Setting {
            const REPRESENTATION: RepresentationId = REPRESENTATION;
        }
    }

    /// Stands in for the payload crate's 2.x release: a distinct type with the same source.
    mod v2 {
        use super::*;

        #[derive(Debug)]
        pub(super) struct Setting(pub(super) &'static str);

        pub(super) const REPRESENTATION: RepresentationId =
            RepresentationId::new("test-payloads", "2", 1);

        impl ConfigPayloadFor<TestSlot> for Setting {
            const REPRESENTATION: RepresentationId = REPRESENTATION;
        }

        impl ConfigPayloadFor<TestListSlot> for Setting {
            const REPRESENTATION: RepresentationId = REPRESENTATION;
        }
    }

    /// Claims v1's representation without being v1's type, as a second linked copy of v1 would.
    #[derive(Debug)]
    struct DuplicateV1;

    impl ConfigPayloadFor<TestSlot> for DuplicateV1 {
        const REPRESENTATION: RepresentationId = v1::REPRESENTATION;
    }

    type TestValue = VersionedConfigValue<TestSlot>;
    type TestListValue = VersionedConfigValue<TestListSlot>;

    fn test_value<T: ConfigPayloadFor<TestSlot>>(payload: T) -> TestValue {
        TestValue::from_arc(Arc::new(payload))
    }

    fn test_list_value<T: ConfigPayloadFor<TestListSlot>>(payload: T) -> TestListValue {
        TestListValue::from_arc(Arc::new(payload))
    }

    fn read(value: &TestValue) -> String {
        if let Some(setting) = value.downcast_ref::<v1::Setting>() {
            format!("v1:{}", setting.0)
        } else if let Some(setting) = value.downcast_ref::<v2::Setting>() {
            format!("v2:{}", setting.0)
        } else {
            panic!("unexpected representation {}", value.representation())
        }
    }

    #[test]
    fn representations_from_incompatible_versions_share_one_slot() {
        let mut lower = Layer::new("lower");
        lower.store_put(test_value(v1::Setting("old")));
        let mut upper = Layer::new("upper");
        upper.store_put(test_value(v2::Setting("new")));
        let cfg = ConfigBag::of_layers(vec![lower, upper]);

        let value = cfg.load::<TestValue>().expect("set");
        assert_eq!("v2:new", read(value));
        assert_eq!(v2::REPRESENTATION, value.representation());
        assert!(value.downcast_ref::<v2::Setting>().is_some());
        assert!(value.downcast_ref::<v1::Setting>().is_none());
        assert!(value.downcast_ref::<v1::Setting>().is_none());
    }

    #[test]
    fn lower_layer_representation_is_visible_when_not_overridden() {
        let mut lower = Layer::new("lower");
        lower.store_put(test_value(v1::Setting("old")));
        let cfg = ConfigBag::of_layers(vec![lower, Layer::new("upper")]);
        assert_eq!("v1:old", read(cfg.load::<TestValue>().unwrap()));
    }

    #[test]
    fn unset_suppresses_every_representation() {
        let mut lower = Layer::new("lower");
        lower.store_put(test_value(v1::Setting("old")));
        let mut upper = Layer::new("upper");
        upper.unset::<TestValue>();
        let cfg = ConfigBag::of_layers(vec![lower, upper]);
        assert!(cfg.load::<TestValue>().is_none());

        let mut lower = Layer::new("lower");
        lower.store_put(test_value(v2::Setting("new")));
        let mut upper = Layer::new("upper");
        upper.store_or_unset::<TestValue>(None);
        let cfg = ConfigBag::of_layers(vec![lower, upper]);
        assert!(cfg.load::<TestValue>().is_none());
    }

    #[test]
    fn append_slot_collects_and_clears_across_representations() {
        let mut first = Layer::new("first");
        first.store_append(test_list_value(v1::Setting("a")));
        let mut second = Layer::new("second");
        second.store_append(test_list_value(v2::Setting("b")));
        second.store_append(test_list_value(v1::Setting("c")));
        let cfg = ConfigBag::of_layers(vec![first, second]);

        let reps: Vec<_> = cfg
            .load::<TestListValue>()
            .map(|v| v.representation().compatibility_line())
            .collect();
        assert_eq!(vec!["1", "2", "1"], reps);

        let mut cleared = Layer::new("cleared");
        cleared.clear::<TestListValue>();
        let mut first = Layer::new("first");
        first.store_append(test_list_value(v1::Setting("a")));
        let cfg = ConfigBag::of_layers(vec![first, cleared]);
        assert_eq!(0, cfg.load::<TestListValue>().count());
    }

    #[test]
    fn clones_share_the_payload_and_work_in_cloneable_layers() {
        let value = test_value(v1::Setting("shared"));
        let clone = value.clone();
        assert!(Arc::ptr_eq(
            &value.downcast_arc::<v1::Setting>().unwrap(),
            &clone.downcast_arc::<v1::Setting>().unwrap(),
        ));
        assert!(value.downcast_arc::<v2::Setting>().is_none());

        let mut layer = CloneableLayer::new("cloneable");
        layer.store_put(value);
        let cfg = ConfigBag::of_layers(vec![layer.clone().into()]);
        assert_eq!("v1:shared", read(cfg.load::<TestValue>().unwrap()));
    }

    #[test]
    fn from_arc_shares_the_callers_allocation() {
        let payload = Arc::new(v2::Setting("arc"));
        let value = TestValue::from_arc(payload.clone());
        assert!(Arc::ptr_eq(
            &payload,
            &value.downcast_arc::<v2::Setting>().unwrap()
        ));
    }

    #[test]
    fn missing_error() {
        let err = ConfigSlotError::missing::<TestSlot>();
        assert!(err.is_missing());
        assert_eq!(None, err.found());
        assert_eq!("test setting", err.slot_name());
        assert_eq!("no test setting is configured", err.to_string());
    }

    #[test]
    fn unsupported_error_lists_what_the_consumer_supports() {
        static SUPPORTED: [RepresentationId; 1] = [v1::REPRESENTATION];
        let value = test_value(v2::Setting("new"));
        let err = value.unsupported_error(&SUPPORTED);
        assert!(err.is_unsupported());
        assert_eq!(Some(v2::REPRESENTATION), err.found());
        assert_eq!(
            "the configured test setting was built with test-payloads@2 (api revision 1), \
             but this client supports [test-payloads@1 (api revision 1)]",
            err.to_string()
        );
    }

    #[test]
    fn supported_representation_that_fails_to_downcast_is_a_mismatch() {
        static SUPPORTED: [RepresentationId; 2] = [v1::REPRESENTATION, v2::REPRESENTATION];
        let value = test_value(DuplicateV1);
        assert_eq!(v1::REPRESENTATION, value.representation());
        assert!(value.downcast_ref::<v1::Setting>().is_none());

        let err = value.unsupported_error(&SUPPORTED);
        assert!(err.is_representation_mismatch());
        assert!(!err.is_unsupported());
        assert!(
            err.to_string().contains("two copies of test-payloads"),
            "{err}"
        );
    }

    #[test]
    fn compatibility_lines_follow_cargo_semver() {
        assert_eq!("1", RepresentationId::compatibility_line_for("1", "1.4"));
        assert_eq!("12", RepresentationId::compatibility_line_for("12", "12.0"));
        assert_eq!(
            "0.61",
            RepresentationId::compatibility_line_for("0", "0.61")
        );
    }

    #[test]
    fn representation_id_macro_uses_the_calling_crates_metadata() {
        const ID: RepresentationId = crate::representation_id!(3);
        assert_eq!("aws-smithy-runtime-api", ID.package());
        assert_eq!(env!("CARGO_PKG_VERSION_MAJOR"), ID.compatibility_line());
        assert_eq!(3, ID.api_revision());
    }

    #[test]
    fn representation_id_display() {
        assert_eq!(
            "aws-smithy-eventstream@0.61 (api revision 2)",
            RepresentationId::new("aws-smithy-eventstream", "0.61", 2).to_string()
        );
    }

    #[test]
    fn debug_names_the_slot_and_representation() {
        let debug = format!("{:?}", test_value(v1::Setting("dbg")));
        assert!(debug.contains("test setting"), "{debug}");
        assert!(debug.contains("test-payloads"), "{debug}");
        assert!(debug.contains("dbg"), "{debug}");
    }

    #[test]
    fn public_types_are_thread_safe_and_errors_are_standard() {
        fn assert_send_sync_static<T: Send + Sync + 'static>() {}
        fn assert_error<T: StdError + Send + Sync + 'static>() {}
        assert_send_sync_static::<TestValue>();
        assert_send_sync_static::<RepresentationId>();
        assert_error::<ConfigSlotError>();
    }
}
