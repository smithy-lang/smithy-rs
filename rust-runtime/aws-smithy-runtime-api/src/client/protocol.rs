/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Version-stable storage for a client protocol selected through configuration.
//!
//! Client protocols are defined by `aws-smithy-schema`, whose API can be expected to change across
//! major versions. A [`ConfigBag`] is keyed by [`TypeId`](std::any::TypeId), so storing a schema
//! type directly would make the bag entry, and every public setter that produces it, change identity
//! whenever schema takes a major version. [`ConfiguredProtocol`] is the type that is stored and
//! passed around instead. It lives in this crate, never changes identity, and holds whichever
//! protocol handle the configuring code provided:
//!
//! - The crate that defines the protocol handle (today `aws-smithy-schema` 1.x) implements
//!   [`ProtocolHandle`] for it, converts it into a `ConfiguredProtocol`, and recovers it with
//!   [`ConfiguredProtocol::downcast_ref`]. `aws-smithy-schema`'s handle is itself an enum with one
//!   variant per version of its protocol trait, so that trait can also evolve within one major
//!   version.
//! - A later major version of that crate implements `ProtocolHandle` for its own handle type. It can
//!   recognize and adapt handles from earlier versions, and a client that only understands an earlier
//!   version reports which version it found instead of silently ignoring the setting.
//! - Code that only needs version-independent capabilities, such as the orchestrator applying a
//!   resolved endpoint, calls them on `ConfiguredProtocol` without depending on the defining crate.
//!
//! This is an open-ended version of a `enum { V1(..), V2(..) }`: each defining crate adds its own
//! "variant" without this crate having to depend on, or be released for, every version.

use crate::box_error::BoxError;
use crate::client::orchestrator::HttpRequest;
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use aws_smithy_types::endpoint::Endpoint;
use std::any::Any;
use std::fmt;
use std::sync::Arc;

/// A client protocol handle that can be stored in a [`ConfiguredProtocol`].
///
/// This is implemented by crates that define client protocols, not by individual protocols:
/// `aws-smithy-schema` implements it for its `SchemaProtocol` enum, which has one variant per
/// version of its client protocol trait. Methods added to this trait in later releases will have
/// default implementations.
pub trait ProtocolHandle: Any + Send + Sync + fmt::Debug {
    /// Identifies the crate and major version that defined this handle type, for example
    /// `aws-smithy-schema 1.x`.
    ///
    /// Used in error messages when a client finds a protocol it cannot use. Defaults to the
    /// handle's type name.
    fn origin(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Applies a resolved endpoint to a request this protocol serialized.
    ///
    /// The orchestrator calls this after endpoint resolution, so that it can delegate endpoint
    /// application to the protocol without depending on the crate that defines it.
    fn update_endpoint(
        &self,
        request: &mut HttpRequest,
        endpoint: &Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), BoxError>;
}

/// A client protocol selected through configuration, stored in a [`ConfigBag`].
///
/// See the [module documentation](self) for why this type exists. Construct one from a
/// protocol-defining crate's handle type, for example with
/// `ConfiguredProtocol::from(SharedClientProtocol::new(protocol))`.
#[derive(Clone, Debug)]
pub struct ConfiguredProtocol(Arc<dyn ProtocolHandle>);

impl ConfiguredProtocol {
    /// Wraps a protocol handle.
    pub fn new(handle: impl ProtocolHandle) -> Self {
        Self(Arc::new(handle))
    }

    /// Returns the wrapped handle if it is a `T`.
    ///
    /// Protocol-defining crates use this to recover their own handle type. `None` means the
    /// protocol was configured with a handle from a different crate or major version; see
    /// [`origin`](Self::origin).
    pub fn downcast_ref<T: ProtocolHandle>(&self) -> Option<&T> {
        let handle: &dyn Any = &*self.0;
        handle.downcast_ref::<T>()
    }

    /// Identifies the crate and major version that defined the wrapped handle.
    pub fn origin(&self) -> &'static str {
        self.0.origin()
    }

    /// Applies a resolved endpoint to a request the wrapped protocol serialized.
    pub fn update_endpoint(
        &self,
        request: &mut HttpRequest,
        endpoint: &Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        self.0.update_endpoint(request, endpoint, cfg)
    }
}

impl Storable for ConfiguredProtocol {
    type Storer = StoreReplace<Self>;
}

impl<T: ProtocolHandle> From<T> for ConfiguredProtocol {
    fn from(handle: T) -> Self {
        Self::new(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_types::config_bag::Layer;

    #[derive(Debug)]
    struct HandleV1(&'static str);

    impl ProtocolHandle for HandleV1 {
        fn origin(&self) -> &'static str {
            "test-protocols 1.x"
        }

        fn update_endpoint(
            &self,
            request: &mut HttpRequest,
            endpoint: &Endpoint,
            _cfg: &ConfigBag,
        ) -> Result<(), BoxError> {
            request.set_uri(endpoint.url())?;
            request.headers_mut().insert("x-handle", self.0);
            Ok(())
        }
    }

    #[derive(Debug)]
    struct HandleV2;

    impl ProtocolHandle for HandleV2 {
        fn update_endpoint(
            &self,
            _request: &mut HttpRequest,
            _endpoint: &Endpoint,
            _cfg: &ConfigBag,
        ) -> Result<(), BoxError> {
            Err("v2 does not apply endpoints".into())
        }
    }

    fn bag_with(protocol: ConfiguredProtocol) -> ConfigBag {
        let mut layer = Layer::new("test");
        layer.store_put(protocol);
        ConfigBag::of_layers(vec![layer])
    }

    #[test]
    fn downcasts_only_to_the_stored_handle_type() {
        let cfg = bag_with(ConfiguredProtocol::new(HandleV1("one")));
        let configured = cfg.load::<ConfiguredProtocol>().expect("stored");
        assert_eq!("one", configured.downcast_ref::<HandleV1>().unwrap().0);
        assert!(configured.downcast_ref::<HandleV2>().is_none());
    }

    #[test]
    fn origin_comes_from_the_handle_and_defaults_to_its_type_name() {
        assert_eq!(
            "test-protocols 1.x",
            ConfiguredProtocol::new(HandleV1("one")).origin()
        );
        assert!(ConfiguredProtocol::new(HandleV2)
            .origin()
            .ends_with("HandleV2"));
    }

    #[test]
    fn update_endpoint_dispatches_without_knowing_the_handle_type() {
        let cfg = bag_with(ConfiguredProtocol::new(HandleV1("one")));
        let configured = cfg.load::<ConfiguredProtocol>().unwrap();
        let mut request = HttpRequest::empty();
        let endpoint = Endpoint::builder().url("https://example.com/").build();
        configured
            .update_endpoint(&mut request, &endpoint, &cfg)
            .unwrap();
        assert_eq!("https://example.com/", request.uri());
        assert_eq!(Some("one"), request.headers().get("x-handle"));

        let v2 = ConfiguredProtocol::new(HandleV2);
        assert!(v2.update_endpoint(&mut request, &endpoint, &cfg).is_err());
    }

    #[test]
    fn clones_share_the_handle() {
        let a = ConfiguredProtocol::new(HandleV1("one"));
        let b = a.clone();
        assert!(std::ptr::eq(
            a.downcast_ref::<HandleV1>().unwrap(),
            b.downcast_ref::<HandleV1>().unwrap()
        ));
    }
}
