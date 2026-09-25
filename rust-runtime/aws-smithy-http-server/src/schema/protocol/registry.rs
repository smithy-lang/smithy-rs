/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Resolution from a service schema to its [`SharedServerProtocol`].
//!
//! Generated service builders resolve their protocol through a [`ProtocolRegistry`] instead of
//! naming a concrete protocol struct, so a protocol implemented outside this crate joins the
//! schema-serde path by contributing a [`ProtocolRegistration`].
//!
//! A service declaring several protocols is served by all of them. The routing service asks them
//! in priority order: the built-ins as [`BUILTIN_PRIORITY`] lists them, other protocols after the
//! built-ins, and protocols that read the body to route after those that route on metadata alone.
//! A registration moves its protocol relative to another with [`ProtocolOrder`].

use aws_smithy_schema::ServiceSchema;

use super::SharedServerProtocol;

/// The built-in protocols in the order a multi-protocol service asks them to claim a request.
pub(crate) const BUILTIN_PRIORITY: [&str; 5] = [
    "smithy.protocols#rpcv2Cbor",
    "aws.protocols#awsJson1_0",
    "aws.protocols#awsJson1_1",
    "aws.protocols#restJson1",
    "aws.protocols#restXml",
];

/// Places a protocol relative to another protocol, by shape ID, in a multi-protocol service's
/// priority order. A constraint naming a protocol the service does not serve is ignored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProtocolOrder {
    /// Ask this protocol before the named one.
    Before(&'static str),
    /// Ask this protocol after the named one.
    After(&'static str),
}

/// A single protocol's entry in a [`ProtocolRegistry`].
///
/// Wraps a function that inspects a service schema and, when the schema carries the protocol's
/// trait ID, produces the protocol. Const-constructible, so registrations can live in statics.
#[derive(Clone, Copy)]
pub struct ProtocolRegistration {
    build: fn(&'static ServiceSchema<'static>) -> Option<SharedServerProtocol>,
    order: &'static [ProtocolOrder],
}

impl ProtocolRegistration {
    /// Creates a protocol registration.
    pub const fn new(build: fn(&'static ServiceSchema<'static>) -> Option<SharedServerProtocol>) -> Self {
        Self { build, order: &[] }
    }

    /// Constrains where the protocol sits in a multi-protocol service's priority order.
    pub const fn with_order(mut self, order: &'static [ProtocolOrder]) -> Self {
        self.order = order;
        self
    }

    pub(crate) fn order(&self) -> &'static [ProtocolOrder] {
        self.order
    }

    fn resolve(&self, service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
        (self.build)(service_schema)
    }
}

impl std::fmt::Debug for ProtocolRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtocolRegistration").finish_non_exhaustive()
    }
}

/// An ordered collection of [`ProtocolRegistration`]s.
///
/// [`ProtocolRegistry::resolve`] asks each registration in order and returns the first protocol
/// produced. Registrations added with [`ProtocolRegistry::register`] are asked before the ones
/// already present, so a later registration overrides a built-in for the same protocol trait.
#[derive(Debug)]
pub struct ProtocolRegistry {
    registrations: Vec<ProtocolRegistration>,
}

impl ProtocolRegistry {
    /// Creates a registry of the built-in protocols: restJson1, restXml, awsJson1.0, awsJson1.1
    /// and rpcv2Cbor.
    pub fn builtin() -> Self {
        Self {
            registrations: vec![
                ProtocolRegistration::new(rpc_v2_cbor_registration),
                ProtocolRegistration::new(aws_json_11_registration),
                ProtocolRegistration::new(aws_json_10_registration),
                ProtocolRegistration::new(rest_json_1_registration),
                ProtocolRegistration::new(rest_xml_registration),
            ],
        }
    }

    /// Adds a registration, giving it precedence over the ones already present.
    pub fn register(mut self, registration: ProtocolRegistration) -> Self {
        self.registrations.insert(0, registration);
        self
    }

    /// Resolves the protocol for `service_schema`: the first registration whose protocol trait
    /// appears on the schema wins. `None` when no registration recognizes the schema.
    pub fn resolve(&self, service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
        self.registrations
            .iter()
            .find_map(|registration| registration.resolve(service_schema))
    }

    /// Resolves the protocol with shape ID `protocol_id` for `service_schema`, honoring overrides
    /// the same way [`Self::resolve`] does.
    pub fn resolve_id(
        &self,
        service_schema: &'static ServiceSchema<'static>,
        protocol_id: &str,
    ) -> Option<SharedServerProtocol> {
        self.registrations
            .iter()
            .filter_map(|registration| registration.resolve(service_schema))
            .find(|protocol| protocol.protocol_id().as_str() == protocol_id)
    }

    /// Resolves every protocol the registrations recognize on `service_schema`, each with its
    /// registration's ordering constraints. When several registrations produce the same protocol,
    /// the first one wins, as in [`Self::resolve`].
    pub(crate) fn resolve_all(
        &self,
        service_schema: &'static ServiceSchema<'static>,
    ) -> Vec<(SharedServerProtocol, &'static [ProtocolOrder])> {
        let mut resolved: Vec<(SharedServerProtocol, &'static [ProtocolOrder])> = Vec::new();
        for registration in &self.registrations {
            let Some(protocol) = registration.resolve(service_schema) else {
                continue;
            };
            if !resolved
                .iter()
                .any(|(existing, _)| existing.protocol_id() == protocol.protocol_id())
            {
                resolved.push((protocol, registration.order()));
            }
        }
        resolved
    }
}

fn has_protocol(service_schema: &'static ServiceSchema<'static>, id: &str) -> bool {
    service_schema
        .protocols()
        .iter()
        .any(|protocol| protocol.as_str() == id)
}

fn rest_json_1_registration(service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
    has_protocol(service_schema, "aws.protocols#restJson1")
        .then(|| SharedServerProtocol::new(crate::protocol::rest_json_1::RestJson1Protocol::default()))
}

fn rest_xml_registration(service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
    has_protocol(service_schema, "aws.protocols#restXml")
        .then(|| SharedServerProtocol::new(crate::protocol::rest_xml::RestXmlProtocol::default()))
}

fn aws_json_10_registration(service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
    has_protocol(service_schema, "aws.protocols#awsJson1_0")
        .then(|| SharedServerProtocol::new(crate::protocol::aws_json_10::AwsJson1_0Protocol::default()))
}

fn aws_json_11_registration(service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
    has_protocol(service_schema, "aws.protocols#awsJson1_1")
        .then(|| SharedServerProtocol::new(crate::protocol::aws_json_11::AwsJson1_1Protocol::default()))
}

fn rpc_v2_cbor_registration(service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
    has_protocol(service_schema, "smithy.protocols#rpcv2Cbor")
        .then(|| SharedServerProtocol::new(crate::protocol::rpc_v2_cbor::RpcV2CborProtocol::default()))
}

#[cfg(test)]
mod tests {
    use aws_smithy_schema::{shape_id, Schema, ServiceSchema, ShapeId, ShapeType};

    use super::*;

    static SERVICE_SHAPE: Schema<'static> = Schema::new(shape_id!("example", "Service"), ShapeType::Service);

    static REST_JSON_1: [ShapeId<'static>; 1] = [shape_id!("aws.protocols", "restJson1")];
    static REST_XML: [ShapeId<'static>; 1] = [shape_id!("aws.protocols", "restXml")];
    static AWS_JSON_10: [ShapeId<'static>; 1] = [shape_id!("aws.protocols", "awsJson1_0")];
    static AWS_JSON_11: [ShapeId<'static>; 1] = [shape_id!("aws.protocols", "awsJson1_1")];
    static RPC_V2_CBOR: [ShapeId<'static>; 1] = [shape_id!("smithy.protocols", "rpcv2Cbor")];
    static UNKNOWN: [ShapeId<'static>; 1] = [shape_id!("example.protocols", "myProtocol")];

    static REST_JSON_1_SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, &REST_JSON_1, &[]);
    static REST_XML_SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, &REST_XML, &[]);
    static AWS_JSON_10_SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, &AWS_JSON_10, &[]);
    static AWS_JSON_11_SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, &AWS_JSON_11, &[]);
    static RPC_V2_CBOR_SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, &RPC_V2_CBOR, &[]);
    static UNKNOWN_SERVICE: ServiceSchema<'static> = ServiceSchema::new(&SERVICE_SHAPE, None, &UNKNOWN, &[]);

    #[test]
    fn builtin_resolves_each_builtin_protocol() {
        let registry = ProtocolRegistry::builtin();
        for (service, id) in [
            (&REST_JSON_1_SERVICE, "aws.protocols#restJson1"),
            (&REST_XML_SERVICE, "aws.protocols#restXml"),
            (&AWS_JSON_10_SERVICE, "aws.protocols#awsJson1_0"),
            (&AWS_JSON_11_SERVICE, "aws.protocols#awsJson1_1"),
            (&RPC_V2_CBOR_SERVICE, "smithy.protocols#rpcv2Cbor"),
        ] {
            let protocol = registry.resolve(service).expect(id);
            assert_eq!(protocol.protocol_id().as_str(), id);
        }
    }

    #[test]
    fn unknown_protocol_resolves_to_none() {
        assert!(ProtocolRegistry::builtin().resolve(&UNKNOWN_SERVICE).is_none());
    }

    #[test]
    fn additional_registration_resolves_an_unknown_protocol() {
        fn my_protocol(service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
            super::has_protocol(service_schema, "example.protocols#myProtocol")
                .then(|| SharedServerProtocol::new(crate::protocol::rest_json_1::RestJson1Protocol::default()))
        }

        let registry = ProtocolRegistry::builtin().register(ProtocolRegistration::new(my_protocol));
        assert!(registry.resolve(&UNKNOWN_SERVICE).is_some());
        // The built-ins still resolve behind it.
        assert!(registry.resolve(&REST_XML_SERVICE).is_some());
    }

    #[test]
    fn additional_registration_overrides_a_builtin() {
        fn override_rest_json_1(service_schema: &'static ServiceSchema<'static>) -> Option<SharedServerProtocol> {
            super::has_protocol(service_schema, "aws.protocols#restJson1")
                .then(|| SharedServerProtocol::new(crate::protocol::rest_xml::RestXmlProtocol::default()))
        }

        let registry = ProtocolRegistry::builtin().register(ProtocolRegistration::new(override_rest_json_1));
        let protocol = registry.resolve(&REST_JSON_1_SERVICE).unwrap();
        assert_eq!(protocol.protocol_id().as_str(), "aws.protocols#restXml");
    }
}
