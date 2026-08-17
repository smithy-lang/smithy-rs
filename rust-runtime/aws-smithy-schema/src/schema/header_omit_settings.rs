/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Runtime suppression of protocol-default request headers.
//!
//! [`SharedHeaderOmitSettings`] is the [`Storable`] config-bag entry that the
//! runtime (notably
//! [`HttpBindingProtocol::serialize_request_with_body`](crate::http_protocol::HttpBindingProtocol::serialize_request_with_body))
//! consults when deciding whether to insert protocol-default `Content-Type` and
//! `Content-Length` headers on outgoing requests. The most common producer is
//! the SigV4 presigning interceptor, which omits both so they don't end up in
//! the signed-header set of presigned URLs.
//!
//! Customer-supplied `@httpHeader` values are unaffected by these settings;
//! they always take priority over the runtime's defaults regardless of the
//! omit flags.
//!
//! The trait/wrapper split mirrors `SharedClientProtocol` — the trait is the
//! contract any caller can implement, the wrapper is the thing actually placed
//! in the [`ConfigBag`](aws_smithy_types::config_bag::ConfigBag).

// IMPLEMENTATION NOTE — why a trait + `Arc`'d wrapper rather than just
// promoting the inlineable `HeaderSerializationSettings` to `pub`?
//
// `HeaderSerializationSettings` lives in `rust-runtime/inlineable/` (and the
// AWS-side duplicate) where it's `pub(crate)` and gets inlined into each
// generated SDK crate. The runtime here needs to read the omit flags from
// the `ConfigBag`, and `ConfigBag::load::<T>()` is `TypeId`-keyed — so the
// producer (presigning interceptor) and the consumer (runtime) must
// reference the same Rust type.
//
// Promoting the inlineable type to `pub` would solve the lookup, but would
// pin a concrete data shape into a published crate's public API. Instead,
// this module publishes only the abstract surface (the trait + an `Arc`'d
// wrapper for `Storable`). Inlineable implements the trait on its existing
// `HeaderSerializationSettings`; new fields there stay a private inlineable
// concern. Adding a new omit category requires a new trait method here,
// which is forward-compatible thanks to default `false` bodies.

use aws_smithy_types::config_bag::{Storable, StoreReplace};
use std::sync::Arc;

/// Configures whether the runtime should suppress protocol-default request
/// headers during serialization.
///
/// All methods default to returning `false` so future trait additions do not
/// break existing implementors.
pub trait HeaderOmitSettings: Send + Sync + std::fmt::Debug {
    /// Returns `true` if the runtime must not insert a default `Content-Type`
    /// header. A customer-supplied `@httpHeader("Content-Type")` value still
    /// takes priority and is unaffected by this flag.
    fn should_omit_default_content_type(&self) -> bool {
        false
    }

    /// Returns `true` if the runtime must not insert a default `Content-Length`
    /// header.
    fn should_omit_default_content_length(&self) -> bool {
        false
    }
}

/// A shared, type-erased [`HeaderOmitSettings`] suitable for storage in the
/// [`ConfigBag`](aws_smithy_types::config_bag::ConfigBag).
///
/// Wraps `Arc<dyn HeaderOmitSettings>`. Cheaply [`Clone`]able via the inner
/// `Arc`.
#[derive(Debug, Clone)]
pub struct SharedHeaderOmitSettings {
    inner: Arc<dyn HeaderOmitSettings>,
}

impl SharedHeaderOmitSettings {
    /// Wraps any [`HeaderOmitSettings`] implementation in a shared, type-erased
    /// container.
    pub fn new<T>(settings: T) -> Self
    where
        T: HeaderOmitSettings + 'static,
    {
        Self {
            inner: Arc::new(settings),
        }
    }

    /// Constructs from an existing `Arc<dyn HeaderOmitSettings>`. Useful when
    /// the same settings instance must be shared across multiple config-bag
    /// entries without re-allocating.
    pub fn from_arc(inner: Arc<dyn HeaderOmitSettings>) -> Self {
        Self { inner }
    }
}

impl std::ops::Deref for SharedHeaderOmitSettings {
    type Target = dyn HeaderOmitSettings;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl Storable for SharedHeaderOmitSettings {
    type Storer = StoreReplace<Self>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_types::config_bag::{ConfigBag, Layer};

    #[derive(Debug, Default)]
    struct OmitBoth;
    impl HeaderOmitSettings for OmitBoth {
        fn should_omit_default_content_type(&self) -> bool {
            true
        }
        fn should_omit_default_content_length(&self) -> bool {
            true
        }
    }

    #[derive(Debug, Default)]
    struct OmitNone;
    impl HeaderOmitSettings for OmitNone {}

    #[test]
    fn default_trait_methods_return_false() {
        let settings = OmitNone;
        assert!(!settings.should_omit_default_content_type());
        assert!(!settings.should_omit_default_content_length());
    }

    #[test]
    fn shared_delegates_to_inner_via_deref() {
        let shared = SharedHeaderOmitSettings::new(OmitBoth);
        assert!(shared.should_omit_default_content_type());
        assert!(shared.should_omit_default_content_length());
    }

    #[test]
    fn shared_round_trips_through_config_bag() {
        let mut layer = Layer::new("test");
        layer.store_put(SharedHeaderOmitSettings::new(OmitBoth));
        let cfg = ConfigBag::of_layers(vec![layer]);

        let loaded = cfg
            .load::<SharedHeaderOmitSettings>()
            .expect("settings stored in ConfigBag");
        assert!(loaded.should_omit_default_content_type());
        assert!(loaded.should_omit_default_content_length());
    }

    #[test]
    fn shared_absent_from_config_bag_means_no_omits() {
        let cfg = ConfigBag::base();
        assert!(cfg.load::<SharedHeaderOmitSettings>().is_none());
    }

    #[test]
    fn from_arc_avoids_realloc_when_caller_already_holds_arc() {
        let original: Arc<dyn HeaderOmitSettings> = Arc::new(OmitBoth);
        let strong = Arc::strong_count(&original);
        let shared = SharedHeaderOmitSettings::from_arc(Arc::clone(&original));
        assert_eq!(Arc::strong_count(&original), strong + 1);
        assert!(shared.should_omit_default_content_type());
    }
}
