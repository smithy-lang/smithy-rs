/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Resolution from a service schema to its [`SharedServerProtocol`]s.
//!
//! Generated service builders resolve their protocols through [`ProtocolRegistry`]s instead of
//! naming concrete protocol structs, so a protocol implemented outside this crate joins the
//! schema-serde path by contributing a registry. A registry is a const collection of
//! [`ProtocolRegistration`]s, so a crate keeps every protocol it provides — and each protocol's
//! [`ProtocolOrder`] placement — in one place:
//!
//! ```ignore
//! pub static MY_PROTOCOLS: ProtocolRegistry = ProtocolRegistry::new(&[
//!     ProtocolRegistration::metadata_routed::<MyProtocol>("example.protocols#myProtocol")
//!         .with_order(&[ProtocolOrder::Before("aws.protocols#restJson1")]),
//! ]);
//! ```
//!
//! A protocol registers as the routing kind it implements:
//! [`ProtocolRegistration::metadata_routed`] for a
//! [`MetadataRoutedProtocol`](super::MetadataRoutedProtocol),
//! [`ProtocolRegistration::body_routed`] for a
//! [`BodyRoutedProtocol`](super::BodyRoutedProtocol).
//!
//! A service declaring several protocols is served by all of them. The claim order is decided
//! only by [`ProtocolOrder`] constraints, resolved over every registered protocol — including
//! ones the service does not serve, so `a Before b` and `b Before c` order `a` before `c` on a
//! service serving only `a` and `c`. Registry and declaration order carry no meaning: two served
//! protocols that the constraints leave unordered fail the build, as does the same protocol
//! registered twice.

use aws_smithy_types::Document;

use crate::schema::routing::RouterBuildError;
use crate::schema::ServiceSchema;

use super::{BodyRoutedProtocol, MetadataRoutedProtocol, SharedServerProtocol};

/// Places a protocol relative to another protocol, by shape ID, in a multi-protocol service's
/// claim order. Constraints resolve transitively over every registered protocol, so a constraint
/// against a protocol the service does not serve still orders the ones it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolOrder {
    /// Ask this protocol before the named one.
    Before(&'static str),
    /// Ask this protocol after the named one.
    After(&'static str),
}

/// Everything a protocol factory sees.
///
/// `#[non_exhaustive]` — grows without breaking registries.
#[non_exhaustive]
#[derive(Debug)]
pub struct ProtocolBuildContext<'a> {
    /// The service schema the protocol is being built for.
    pub service: &'static ServiceSchema<'static>,
    /// This protocol's own section from `customizationConfig.protocols.<its shape ID>`.
    pub settings: Option<&'a Document>,
    /// The shared section every protocol may read: `customizationConfig.protocols.global`.
    pub global: Option<&'a Document>,
}

impl<'a> ProtocolBuildContext<'a> {
    /// Creates a context with no settings, for tests and manual construction.
    pub fn new(service: &'static ServiceSchema<'static>) -> Self {
        Self {
            service,
            settings: None,
            global: None,
        }
    }

    /// Sets the protocol's own settings section.
    pub fn with_settings(mut self, settings: Option<&'a Document>) -> Self {
        self.settings = settings;
        self
    }

    /// Sets the shared settings section.
    pub fn with_global(mut self, global: Option<&'a Document>) -> Self {
        self.global = global;
        self
    }
}

/// Builds a protocol from its registered configuration into the erased handle carrying its
/// routing kind. Produced by [`ProtocolRegistration::metadata_routed`] and
/// [`ProtocolRegistration::body_routed`], never written by hand.
pub type ProtocolFactory = fn(&ProtocolBuildContext<'_>) -> Result<SharedServerProtocol, RouterBuildError>;

fn build_metadata_routed<P: MetadataRoutedProtocol>(
    ctx: &ProtocolBuildContext<'_>,
) -> Result<SharedServerProtocol, RouterBuildError> {
    Ok(SharedServerProtocol::metadata_routed(P::from_build_context(ctx)?))
}

fn build_body_routed<P: BodyRoutedProtocol>(
    ctx: &ProtocolBuildContext<'_>,
) -> Result<SharedServerProtocol, RouterBuildError> {
    Ok(SharedServerProtocol::body_routed(P::from_build_context(ctx)?))
}

/// A single protocol's entry in a [`ProtocolRegistry`].
///
/// Names the protocol it provides, wraps the factory that builds it, and carries the protocol's
/// [`ProtocolOrder`] placement. Const-constructible, so registrations live in static registries.
#[derive(Clone, Copy)]
pub struct ProtocolRegistration {
    protocol_id: &'static str,
    build: ProtocolFactory,
    order: &'static [ProtocolOrder],
}

impl ProtocolRegistration {
    /// Registers the metadata-routed protocol `P` for the protocol trait `protocol_id`.
    ///
    /// `P` is built only for services whose schema declares `protocol_id`, and must answer the
    /// same ID from [`protocol_id`](super::ServerProtocol::protocol_id).
    pub const fn metadata_routed<P: MetadataRoutedProtocol>(protocol_id: &'static str) -> Self {
        Self {
            protocol_id,
            build: build_metadata_routed::<P>,
            order: &[],
        }
    }

    /// Registers the body-routed protocol `P` for the protocol trait `protocol_id`.
    ///
    /// `P` is built only for services whose schema declares `protocol_id`, and must answer the
    /// same ID from [`protocol_id`](super::ServerProtocol::protocol_id). The routing service
    /// hands `P` only the service's non-streaming operations; see
    /// [`BodyRoutedProtocol`](super::BodyRoutedProtocol).
    pub const fn body_routed<P: BodyRoutedProtocol>(protocol_id: &'static str) -> Self {
        Self {
            protocol_id,
            build: build_body_routed::<P>,
            order: &[],
        }
    }

    /// Constrains where the protocol sits in a multi-protocol service's claim order.
    pub const fn with_order(mut self, order: &'static [ProtocolOrder]) -> Self {
        self.order = order;
        self
    }

    pub(crate) fn protocol_id(&self) -> &'static str {
        self.protocol_id
    }

    pub(crate) fn order(&self) -> &'static [ProtocolOrder] {
        self.order
    }

    pub(crate) fn build(&self, ctx: &ProtocolBuildContext<'_>) -> Result<SharedServerProtocol, RouterBuildError> {
        let protocol = (self.build)(ctx)?;
        if protocol.protocol_id().as_str() != self.protocol_id {
            return Err(RouterBuildError::Configuration(format!(
                "protocol registered as `{}` answers `{}` from `protocol_id()`",
                self.protocol_id,
                protocol.protocol_id()
            )));
        }
        Ok(protocol)
    }
}

impl std::fmt::Debug for ProtocolRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolRegistration")
            .field("protocol_id", &self.protocol_id)
            .finish_non_exhaustive()
    }
}

/// A const collection of [`ProtocolRegistration`]s.
///
/// Each protocol may be registered exactly once across every registry a service consults;
/// a duplicate fails the build. Order within a registry carries no meaning.
#[derive(Clone, Copy, Debug)]
pub struct ProtocolRegistry {
    registrations: &'static [ProtocolRegistration],
}

impl ProtocolRegistry {
    /// The built-in protocols: rpcv2Cbor, awsJson1.0, awsJson1.1, restJson1 and restXml,
    /// chained into a fixed relative claim order by explicit constraints.
    pub const BUILTIN: ProtocolRegistry = ProtocolRegistry::new(&[
        ProtocolRegistration::metadata_routed::<crate::schema::protocol::RpcV2CborProtocol>(
            "smithy.protocols#rpcv2Cbor",
        ),
        ProtocolRegistration::metadata_routed::<crate::schema::protocol::AwsJson1_0Protocol>(
            "aws.protocols#awsJson1_0",
        )
        .with_order(&[ProtocolOrder::After("smithy.protocols#rpcv2Cbor")]),
        ProtocolRegistration::metadata_routed::<crate::schema::protocol::AwsJson1_1Protocol>(
            "aws.protocols#awsJson1_1",
        )
        .with_order(&[ProtocolOrder::After("aws.protocols#awsJson1_0")]),
        ProtocolRegistration::metadata_routed::<crate::schema::protocol::RestJson1Protocol>("aws.protocols#restJson1")
            .with_order(&[ProtocolOrder::After("aws.protocols#awsJson1_1")]),
        ProtocolRegistration::metadata_routed::<crate::schema::protocol::RestXmlProtocol>("aws.protocols#restXml")
            .with_order(&[ProtocolOrder::After("aws.protocols#restJson1")]),
    ]);

    /// Creates a registry over `registrations`.
    pub const fn new(registrations: &'static [ProtocolRegistration]) -> Self {
        Self { registrations }
    }

    pub(crate) fn registrations(&self) -> &'static [ProtocolRegistration] {
        self.registrations
    }

    /// Builds the protocol with shape ID `protocol_id` for `service_schema`, without settings.
    ///
    /// Returns `Ok(None)` when this registry does not register `protocol_id` or the schema does not declare
    /// it. Factory failures and registered-ID mismatches return `Err`.
    /// Used by generated protocol tests, which exercise one protocol in isolation.
    pub fn resolve_id(
        &self,
        service_schema: &'static ServiceSchema<'static>,
        protocol_id: &str,
    ) -> Result<Option<SharedServerProtocol>, RouterBuildError> {
        let declared = service_schema
            .protocols()
            .iter()
            .any(|protocol| protocol.as_str() == protocol_id);
        if !declared {
            return Ok(None);
        }
        self.registrations
            .iter()
            .find(|registration| registration.protocol_id() == protocol_id)
            .map(|registration| registration.build(&ProtocolBuildContext::new(service_schema)))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::schema::ServiceSchema;
    use aws_smithy_schema::{shape_id, ShapeId};

    use super::*;

    const SERVICE_ID: ShapeId<'static> = shape_id!("example", "Service");

    static REST_JSON_1: [ShapeId<'static>; 1] = [shape_id!("aws.protocols", "restJson1")];
    static UNKNOWN: [ShapeId<'static>; 1] = [shape_id!("example.protocols", "myProtocol")];

    static REST_JSON_1_SERVICE: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, &REST_JSON_1, &[]);
    static UNKNOWN_SERVICE: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, &UNKNOWN, &[]);

    #[test]
    fn builtin_resolves_each_builtin_protocol() {
        static PROTOCOLS: [[ShapeId<'static>; 1]; 5] = [
            [shape_id!("aws.protocols", "restJson1")],
            [shape_id!("aws.protocols", "restXml")],
            [shape_id!("aws.protocols", "awsJson1_0")],
            [shape_id!("aws.protocols", "awsJson1_1")],
            [shape_id!("smithy.protocols", "rpcv2Cbor")],
        ];
        static SERVICES: [ServiceSchema<'static>; 5] = [
            ServiceSchema::new(SERVICE_ID, None, &PROTOCOLS[0], &[]),
            ServiceSchema::new(SERVICE_ID, None, &PROTOCOLS[1], &[]),
            ServiceSchema::new(SERVICE_ID, None, &PROTOCOLS[2], &[]),
            ServiceSchema::new(SERVICE_ID, None, &PROTOCOLS[3], &[]),
            ServiceSchema::new(SERVICE_ID, None, &PROTOCOLS[4], &[]),
        ];
        for (service, id) in SERVICES.iter().zip([
            "aws.protocols#restJson1",
            "aws.protocols#restXml",
            "aws.protocols#awsJson1_0",
            "aws.protocols#awsJson1_1",
            "smithy.protocols#rpcv2Cbor",
        ]) {
            let protocol = ProtocolRegistry::BUILTIN
                .resolve_id(service, id)
                .expect("factory succeeds")
                .expect(id);
            assert_eq!(protocol.protocol_id().as_str(), id);
        }
    }

    #[test]
    fn unknown_protocol_resolves_to_none() {
        assert!(ProtocolRegistry::BUILTIN
            .resolve_id(&UNKNOWN_SERVICE, "example.protocols#myProtocol")
            .expect("unavailable protocol is not a build error")
            .is_none());
    }

    #[test]
    fn undeclared_protocol_resolves_to_none() {
        assert!(ProtocolRegistry::BUILTIN
            .resolve_id(&REST_JSON_1_SERVICE, "aws.protocols#restXml")
            .expect("unavailable protocol is not a build error")
            .is_none());
    }

    #[test]
    fn a_mismatched_protocol_id_is_an_error() {
        static REGISTRY: ProtocolRegistry = ProtocolRegistry::new(&[ProtocolRegistration::metadata_routed::<
            crate::schema::protocol::RestXmlProtocol,
        >("aws.protocols#restJson1")]);
        let err = REGISTRY
            .resolve_id(&REST_JSON_1_SERVICE, "aws.protocols#restJson1")
            .expect_err("IDs disagree");
        assert!(matches!(err, RouterBuildError::Configuration(_)));
    }

    #[test]
    fn factory_failure_is_preserved() {
        static REGISTRY: ProtocolRegistry = ProtocolRegistry::new(&[ProtocolRegistration {
            protocol_id: "aws.protocols#restJson1",
            build: |_| Err(RouterBuildError::Configuration("factory failed".into())),
            order: &[],
        }]);
        let err = REGISTRY
            .resolve_id(&REST_JSON_1_SERVICE, "aws.protocols#restJson1")
            .unwrap_err();
        assert!(matches!(err, RouterBuildError::Configuration(message) if message == "factory failed"));
    }
}
