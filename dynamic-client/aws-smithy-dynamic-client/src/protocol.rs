/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

// Protocol detection from the protocol traits of a service.

use aws_smithy_cbor::protocol::RpcV2CborProtocol;
use aws_smithy_json::protocol::aws_json_rpc::AwsJsonRpcProtocol;
use aws_smithy_json::protocol::aws_rest_json_1::AwsRestJsonProtocol;
use aws_smithy_lang::shape::ServiceView;
use aws_smithy_lang::{Model, ShapeId};
use aws_smithy_query::protocol::AwsQueryProtocol;
use aws_smithy_schema::protocol::SharedClientProtocol;
use aws_smithy_xml::protocol::aws_rest_xml::AwsRestXmlProtocol;

const RPC_V2_CBOR: ShapeId = ShapeId::from_static("smithy.protocols#rpcv2Cbor");
const AWS_JSON_1_0: ShapeId = ShapeId::from_static("aws.protocols#awsJson1_0");
const AWS_JSON_1_1: ShapeId = ShapeId::from_static("aws.protocols#awsJson1_1");
const REST_JSON_1: ShapeId = ShapeId::from_static("aws.protocols#restJson1");
const REST_XML: ShapeId = ShapeId::from_static("aws.protocols#restXml");
pub(crate) const AWS_QUERY: ShapeId = ShapeId::from_static("aws.protocols#awsQuery");
const EC2_QUERY: ShapeId = ShapeId::from_static("aws.protocols#ec2Query");
const PROTOCOL_DEFINITION: ShapeId = ShapeId::from_static("smithy.api#protocolDefinition");

// The protocol traits generated clients support. The list includes ec2Query, although it has no
// shared implementation. Models often omit the definitions of protocol traits, so the known traits
// are listed by ID.
const KNOWN_PROTOCOLS: &[ShapeId] = &[
    RPC_V2_CBOR,
    AWS_JSON_1_0,
    AWS_JSON_1_1,
    REST_JSON_1,
    REST_XML,
    AWS_QUERY,
    EC2_QUERY,
];

type CreateProtocol = fn(&ServiceView<'_>) -> SharedClientProtocol;

// The built-in protocols, in the priority order generated clients use, and built the way
// generated clients build them.
const BUILT_IN_PROTOCOLS: &[(ShapeId, CreateProtocol)] = &[
    (RPC_V2_CBOR, |_| {
        SharedClientProtocol::new(RpcV2CborProtocol::new())
    }),
    (AWS_JSON_1_0, |service| {
        SharedClientProtocol::new(
            AwsJsonRpcProtocol::aws_json_1_0().with_default_namespace(namespace(service)),
        )
    }),
    (AWS_JSON_1_1, |service| {
        SharedClientProtocol::new(
            AwsJsonRpcProtocol::aws_json_1_1().with_default_namespace(namespace(service)),
        )
    }),
    (REST_JSON_1, |service| {
        SharedClientProtocol::new(
            AwsRestJsonProtocol::new().with_default_namespace(namespace(service)),
        )
    }),
    (REST_XML, |service| {
        let no_error_wrapping = service
            .get_trait(&REST_XML)
            .and_then(|value| value.as_object())
            .and_then(|value| value.get("noErrorWrapping"))
            .and_then(|value| value.as_bool())
            == Some(true);
        SharedClientProtocol::new(
            AwsRestXmlProtocol::new().with_no_error_wrapping(no_error_wrapping),
        )
    }),
    (AWS_QUERY, |_| {
        SharedClientProtocol::new(AwsQueryProtocol::new())
    }),
];

// The first built-in protocol, in priority order, whose protocol trait the service has.
pub(crate) fn detect_protocol(service: &ServiceView<'_>) -> Option<SharedClientProtocol> {
    BUILT_IN_PROTOCOLS
        .iter()
        .find(|(id, _)| service.has_trait(id))
        .map(|(_, create)| create(service))
}

// The protocol traits on the service, sorted by shape ID. A trait counts as a protocol trait
// when the model defines it with `@protocolDefinition`, or when it is a known protocol trait.
pub(crate) fn protocol_traits(model: &Model, service: &ServiceView<'_>) -> Vec<ShapeId> {
    let defined = |id: &ShapeId| {
        model
            .get_shape(id)
            .is_some_and(|shape| shape.has_trait(&PROTOCOL_DEFINITION))
    };
    let mut traits: Vec<ShapeId> = service
        .traits()
        .ids()
        .filter(|id| KNOWN_PROTOCOLS.contains(id) || defined(id))
        .cloned()
        .collect();
    traits.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    traits
}

fn namespace(service: &ServiceView<'_>) -> String {
    service.id().namespace().to_owned()
}
