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
//! passed around instead.
//!
//! `ConfiguredProtocol` is the [`ClientProtocolSlot`] instance of the general
//! [versioned config](crate::client::versioned_config) mechanism, plus one protocol-specific
//! capability:
//!
//! - The crate that defines the protocol handle (today `aws-smithy-schema` 1.x) implements
//!   [`ProtocolHandle`] and [`ConfigPayloadFor<ClientProtocolSlot>`] for it. The latter records
//!   which crate and compatibility line produced the handle as a [`RepresentationId`].
//!   `aws-smithy-schema`'s handle is itself an enum with one variant per version of its protocol
//!   trait, so that trait can also evolve within one compatibility line.
//! - Clients recover a handle with [`ConfiguredProtocol::downcast_ref`], trying each
//!   representation they support, and report anything else with
//!   [`ConfiguredProtocol::unsupported_error`] instead of treating it as absent.
//! - Code that only needs version-independent capabilities, such as the orchestrator applying a
//!   resolved endpoint, calls them through [`ProtocolHandle`] without depending on the defining
//!   crate.

use crate::box_error::BoxError;
use crate::client::orchestrator::HttpRequest;
use crate::client::versioned_config::{
    config_slot, ConfigPayloadFor, ConfigSlotError, RepresentationId, VersionedConfigValue,
};
use aws_smithy_types::config_bag::{ConfigBag, Storable, StoreReplace};
use aws_smithy_types::endpoint::Endpoint;
use std::fmt;
use std::sync::Arc;

config_slot! {
    /// The stable marker for the client protocol selected through configuration.
    ///
    /// Protocol-defining crates implement [`ConfigPayloadFor<ClientProtocolSlot>`] for their
    /// [`ProtocolHandle`] type. The bag entry for this slot is [`ConfiguredProtocol`].
    pub enum ClientProtocolSlot { name: "client protocol", storage: wrapper }
}

/// The version-independent capabilities of a protocol stored in a [`ConfiguredProtocol`].
///
/// This is implemented by crates that define client protocols, not by individual protocols:
/// `aws-smithy-schema` implements it for its `SchemaProtocol` enum, which has one variant per
/// version of its client protocol trait. Implementors also implement
/// [`ConfigPayloadFor<ClientProtocolSlot>`] to identify their representation. Methods added to this
/// trait in later releases will have default implementations.
pub trait ProtocolHandle: Send + Sync + fmt::Debug + 'static {
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
#[derive(Clone)]
pub struct ConfiguredProtocol {
    value: VersionedConfigValue<ClientProtocolSlot>,
    /// The same allocation as `value`'s payload, viewed through the stable capability trait.
    handle: Arc<dyn ProtocolHandle>,
}

impl ConfiguredProtocol {
    /// Wraps a protocol handle.
    pub fn new<T>(handle: T) -> Self
    where
        T: ProtocolHandle + ConfigPayloadFor<ClientProtocolSlot>,
    {
        let handle = Arc::new(handle);
        Self {
            value: VersionedConfigValue::from_arc(handle.clone()),
            handle,
        }
    }

    /// Identifies the crate and compatibility line that produced the wrapped handle.
    pub fn representation(&self) -> RepresentationId {
        self.value.representation()
    }

    /// Returns the wrapped handle if it is a `T`.
    ///
    /// `None` means the protocol was configured with a handle from a different crate or
    /// compatibility line; see [`representation`](Self::representation) and
    /// [`unsupported_error`](Self::unsupported_error).
    pub fn downcast_ref<T: ConfigPayloadFor<ClientProtocolSlot>>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }

    /// Returns a shared handle to the wrapped handle if it is a `T`.
    pub fn downcast_arc<T: ConfigPayloadFor<ClientProtocolSlot>>(&self) -> Option<Arc<T>> {
        self.value.downcast_arc()
    }

    /// Returns the error for an absent configured client protocol.
    pub fn missing_error() -> ConfigSlotError {
        ConfigSlotError::missing::<ClientProtocolSlot>()
    }

    /// Builds the error for a handle that none of the consumer's `supported` representations could
    /// downcast.
    pub fn unsupported_error(&self, supported: &'static [RepresentationId]) -> ConfigSlotError {
        self.value.unsupported_error(supported)
    }

    /// Applies a resolved endpoint to a request the wrapped protocol serialized.
    pub fn update_endpoint(
        &self,
        request: &mut HttpRequest,
        endpoint: &Endpoint,
        cfg: &ConfigBag,
    ) -> Result<(), BoxError> {
        self.handle.update_endpoint(request, endpoint, cfg)
    }
}

impl fmt::Debug for ConfiguredProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ConfiguredProtocol")
            .field(&self.value)
            .finish()
    }
}

impl Storable for ConfiguredProtocol {
    type Storer = StoreReplace<Self>;
}

impl<T> From<T> for ConfiguredProtocol
where
    T: ProtocolHandle + ConfigPayloadFor<ClientProtocolSlot>,
{
    fn from(handle: T) -> Self {
        Self::new(handle)
    }
}

/// The name of the Smithy `service` shape a client was generated for.
///
/// Some protocols derive parts of the wire format from model names rather than
/// from HTTP binding traits. RPC v2 CBOR is the canonical example: every request
/// is routed to `/service/{serviceName}/operation/{operationName}`, where
/// `serviceName` is the *service shape name* — not the `@aws.api#service`
/// `sdkId`, and not the shape's namespace.
///
/// Because the [`ConfiguredProtocol`] can be swapped at runtime, a protocol cannot
/// rely on codegen having baked its route into the generated request path: a
/// client generated for `awsJson1_0` may have `RpcV2CborProtocol` plugged in via
/// `Config::builder().protocol(..)`. Generated clients therefore store this entry
/// in the config bag regardless of which protocol they were generated for, so
/// whichever protocol ends up being used can resolve the names it needs. The
/// companion operation name comes from
/// [`Metadata::name`](crate::client::orchestrator::Metadata::name).
///
/// See <https://github.com/smithy-lang/smithy-rs/issues/4801>.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceShapeName(std::borrow::Cow<'static, str>);

impl ServiceShapeName {
    /// Creates a new [`ServiceShapeName`] from the Smithy service shape name.
    ///
    /// Accepts a codegen-emitted `&'static str` as well as a `String`
    /// materialized at runtime from a parsed model.
    pub fn new(name: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self(name.into())
    }

    /// Returns the service shape name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Storable for ServiceShapeName {
    type Storer = StoreReplace<Self>;
}

/// The namespace of the Smithy `service` shape a client was generated for.
///
/// This is the `com.amazonaws.dynamodb` in `com.amazonaws.dynamodb#DynamoDB_20120810`.
/// Together with [`ServiceShapeName`] it forms the service's full shape ID; the two are
/// separate entries rather than one because [`ConfigBag`] is keyed by type, so each protocol
/// loads exactly the facts it needs and new facts stay additive.
///
/// **Not to be confused with [`ServiceXmlNamespace`]**, despite the shared word. That one is
/// the `@xmlNamespace` *trait* — a URI restXml applies as the default `xmlns` on root
/// elements — and neither value is derivable from the other. CloudWatch Logs is the clearest
/// illustration: its shape-ID namespace is `com.amazonaws.cloudwatchlogs` while its
/// `@xmlNamespace` URI is `http://monitoring.amazonaws.com/doc/2014-03-28/`. The trait is
/// also optional, carried by roughly half of AWS service shapes, whereas every shape ID has
/// a namespace by construction — which is why this entry is stored unconditionally and
/// `ServiceXmlNamespace` is not.
///
/// Protocols use this as the *default namespace* when resolving a document type's shape
/// discriminator. Some services serialize a discriminator as a bare shape name rather than an
/// absolute shape ID — a `__type` of `Widget` instead of `com.example#Widget` — and the
/// receiving client is expected to qualify it with the service's namespace. Without it a
/// relative discriminator cannot be resolved to a registered type at all.
///
/// Stored for the same reason as [`ServiceShapeName`]: it is knowable only from the model, and
/// a customer may select a different protocol at runtime via `Config::builder().protocol(..)`
/// on a client generated for some other protocol. Baking it into a constructor call at codegen
/// time means a swapped-in protocol either gets no value or gets one the caller had to know to
/// supply. Generated clients therefore store it regardless of which protocol they were
/// generated for.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceShapeNamespace(std::borrow::Cow<'static, str>);

impl ServiceShapeNamespace {
    /// Creates a new [`ServiceShapeNamespace`] from the Smithy service shape's namespace.
    ///
    /// Accepts a codegen-emitted `&'static str` as well as a `String` materialized at runtime
    /// from a parsed model.
    pub fn new(namespace: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self(namespace.into())
    }

    /// Returns the service shape's namespace.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Storable for ServiceShapeNamespace {
    type Storer = StoreReplace<Self>;
}

/// The Smithy service shape's `version`, stored in a [`ConfigBag`] by generated clients.
///
/// awsQuery puts this on the wire as the `Version=` form parameter, so it is a request-shaping
/// fact that only the model knows. Stored for the same reason as [`ServiceShapeName`]: a customer
/// can select awsQuery via `Config::builder().protocol(..)` on a client generated for some other
/// protocol, and could not otherwise supply the right value.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceVersion(std::borrow::Cow<'static, str>);

impl ServiceVersion {
    /// Creates a new [`ServiceVersion`].
    ///
    /// Accepts a codegen-emitted `&'static str` as well as a `String` materialized at runtime
    /// from a parsed model.
    pub fn new(version: impl Into<std::borrow::Cow<'static, str>>) -> Self {
        Self(version.into())
    }

    /// Returns the service version.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Storable for ServiceVersion {
    type Storer = StoreReplace<Self>;
}

/// The service-level `@xmlNamespace` trait, stored in a [`ConfigBag`] by generated clients.
///
/// restXml applies this as the default `xmlns` on request and response root elements, so it is a
/// request-shaping fact that only the model knows. Stored for the same reason as
/// [`ServiceShapeName`].
///
/// `@xmlNamespace` is a prelude trait rather than a restXml-specific one, so it is resolvable from
/// any model — unlike `@restXml(noErrorWrapping)`, which a non-restXml model simply does not carry
/// and which therefore stays caller-supplied.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ServiceXmlNamespace {
    uri: std::borrow::Cow<'static, str>,
    prefix: Option<std::borrow::Cow<'static, str>>,
}

impl ServiceXmlNamespace {
    /// Creates a new [`ServiceXmlNamespace`] from the trait's URI and optional prefix.
    pub fn new(
        uri: impl Into<std::borrow::Cow<'static, str>>,
        prefix: Option<std::borrow::Cow<'static, str>>,
    ) -> Self {
        Self {
            uri: uri.into(),
            prefix,
        }
    }

    /// Returns the namespace URI.
    pub fn uri(&self) -> &str {
        &self.uri
    }

    /// Returns the namespace prefix, if the trait declared one.
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }
}

impl Storable for ServiceXmlNamespace {
    type Storer = StoreReplace<Self>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_smithy_types::config_bag::Layer;

    const V1: RepresentationId = RepresentationId::new("test-protocols", "1", 1);
    const V2: RepresentationId = RepresentationId::new("test-protocols", "2", 1);

    #[derive(Debug)]
    struct HandleV1(&'static str);

    impl ConfigPayloadFor<ClientProtocolSlot> for HandleV1 {
        const REPRESENTATION: RepresentationId = V1;
    }

    impl ProtocolHandle for HandleV1 {
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

    impl ConfigPayloadFor<ClientProtocolSlot> for HandleV2 {
        const REPRESENTATION: RepresentationId = V2;
    }

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
        assert!(configured.downcast_arc::<HandleV1>().is_some());
    }

    #[test]
    fn representation_comes_from_the_payload_contract() {
        assert_eq!(
            V1,
            ConfiguredProtocol::new(HandleV1("one")).representation()
        );
        assert_eq!(V2, ConfiguredProtocol::from(HandleV2).representation());
    }

    #[test]
    fn higher_layer_replaces_a_handle_from_another_compatibility_line() {
        let mut lower = Layer::new("lower");
        lower.store_put(ConfiguredProtocol::new(HandleV1("old")));
        let mut upper = Layer::new("upper");
        upper.store_put(ConfiguredProtocol::new(HandleV2));
        let cfg = ConfigBag::of_layers(vec![lower, upper]);
        assert_eq!(
            V2,
            cfg.load::<ConfiguredProtocol>().unwrap().representation()
        );

        let mut lower = Layer::new("lower");
        lower.store_put(ConfiguredProtocol::new(HandleV1("old")));
        let mut upper = Layer::new("upper");
        upper.unset::<ConfiguredProtocol>();
        let cfg = ConfigBag::of_layers(vec![lower, upper]);
        assert!(cfg.load::<ConfiguredProtocol>().is_none());
    }

    #[test]
    fn unsupported_error_reports_the_found_representation() {
        static SUPPORTED: [RepresentationId; 1] = [V1];
        let err = ConfiguredProtocol::new(HandleV2).unsupported_error(&SUPPORTED);
        assert!(err.is_unsupported());
        assert_eq!(Some(V2), err.found());
        assert_eq!("client protocol", err.slot_name());
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
    fn clones_share_the_handle_and_capability_shares_the_payload() {
        let a = ConfiguredProtocol::new(HandleV1("one"));
        let b = a.clone();
        let payload = a.downcast_arc::<HandleV1>().unwrap();
        assert!(Arc::ptr_eq(
            &payload,
            &b.downcast_arc::<HandleV1>().unwrap()
        ));
        // The capability view and the downcastable payload are one allocation.
        assert!(std::ptr::addr_eq(
            Arc::as_ptr(&a.handle),
            Arc::as_ptr(&payload)
        ));
    }

    #[test]
    fn debug_includes_the_representation() {
        let debug = format!("{:?}", ConfiguredProtocol::new(HandleV1("dbg")));
        assert!(debug.contains("test-protocols"), "{debug}");
        assert!(debug.contains("client protocol"), "{debug}");
    }
}
