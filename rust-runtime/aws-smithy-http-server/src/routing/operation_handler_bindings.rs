/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

use crate::{
    protocol::{
        aws_json::router::AwsJsonRouter,
        aws_json_10::AwsJson1_0,
        aws_json_11::AwsJson1_1,
        rest::router::RestRouter,
        rest_json_1::RestJson1,
        rest_xml::RestXml,
        rpc_v2_cbor::{router::RpcV2CborRouter, RpcV2Cbor},
    },
    routing::{request_spec, RoutingService},
    schema::{OperationSchema, ServiceSchema},
};

/// An operation handler paired with the operation schema it handles.
#[derive(Debug, Clone)]
pub struct OperationHandlerBinding<S> {
    pub operation: &'static OperationSchema<'static>,
    pub handler: S,
}

impl<S> OperationHandlerBinding<S> {
    pub fn new(operation: &'static OperationSchema<'static>, handler: S) -> Self {
        Self { operation, handler }
    }
}

/// Error returned when building a routing service from Smithy schemas.
#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    /// The service schema does not list the protocol requested by the caller.
    #[error("service schema does not list expected protocol `{expected}`")]
    MissingProtocol {
        /// Expected Smithy protocol shape ID.
        expected: &'static str,
    },
    /// Multiple protocol routing tables were registered for the same protocol.
    #[error("multiple server protocols registered for `{protocol}`")]
    DuplicateServerProtocol {
        /// Duplicated Smithy protocol shape ID.
        protocol: String,
    },
    /// Protocol routing order constraints contain a cycle.
    #[error("protocol routing order constraints contain a cycle")]
    ProtocolRoutingOrderCycle,
}

/// Provides a protocol router for operation handler bindings.
///
/// This lets generated code pass protocol-neutral [`OperationHandlerBinding`] values while
/// the protocol marker owns conversion into its concrete router type.
pub trait RouterForOperationHandlerBindings<S>: Sized {
    type Router;

    /// Builds the protocol-specific router from operation handler bindings.
    fn router_for_operation_handler_bindings<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<Self::Router, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>;

    /// Builds a [`RoutingService`] from operation handler bindings.
    fn build_routing_service<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<RoutingService<Self::Router, Self>, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        Ok(RoutingService::new(Self::router_for_operation_handler_bindings(
            service_schema,
            bindings,
        )?))
    }
}

fn ensure_protocol(service_schema: &'static ServiceSchema<'static>, expected: &'static str) -> Result<(), BuildError> {
    if service_schema
        .protocols()
        .iter()
        .any(|protocol| protocol.as_str() == expected)
    {
        Ok(())
    } else {
        Err(BuildError::MissingProtocol { expected })
    }
}

fn router_for_operation_handler_bindings<S, RouterRequestSpec, Router, I, F>(
    service_schema: &'static ServiceSchema<'static>,
    bindings: I,
    expected_protocol: &'static str,
    request_spec: F,
) -> Result<Router, BuildError>
where
    I: IntoIterator<Item = OperationHandlerBinding<S>>,
    Router: FromIterator<(RouterRequestSpec, S)>,
    F: Fn(&'static ServiceSchema<'static>, &'static OperationSchema<'static>) -> RouterRequestSpec,
{
    ensure_protocol(service_schema, expected_protocol)?;
    let router = Router::from_iter(bindings.into_iter().map(|binding| {
        let OperationHandlerBinding { operation, handler } = binding;
        (request_spec(service_schema, operation), handler)
    }));

    Ok(router)
}

pub(crate) fn rest_request_spec(
    _service_schema: &'static ServiceSchema<'static>,
    operation: &'static OperationSchema<'static>,
) -> request_spec::RequestSpec {
    let http_trait = operation
        .schema()
        .http()
        .expect("REST operation schema missing @http trait");
    let method = http_trait
        .method()
        .parse()
        .expect("invalid HTTP method in generated operation schema");
    let (path, query) = http_trait.uri().split_once('?').unwrap_or((http_trait.uri(), ""));

    let path_segments = path
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            if segment.starts_with('{') && segment.ends_with("+}") {
                request_spec::PathSegment::Greedy(segment[1..segment.len() - 2].to_owned())
            } else if segment.starts_with('{') && segment.ends_with('}') {
                request_spec::PathSegment::Label(segment[1..segment.len() - 1].to_owned())
            } else {
                request_spec::PathSegment::Literal(segment.to_owned())
            }
        })
        .collect();

    let query_segments = form_urlencoded::parse(query.as_bytes())
        .map(|(key, value)| {
            if value.is_empty() {
                request_spec::QuerySegment::Key(key.into_owned())
            } else {
                request_spec::QuerySegment::KeyValue(key.into_owned(), value.into_owned())
            }
        })
        .collect();

    request_spec::RequestSpec::new(
        method,
        request_spec::UriSpec::new(request_spec::PathAndQuerySpec::new(
            request_spec::PathSpec::from_vector_unchecked(path_segments),
            request_spec::QuerySpec::from_vector_unchecked(query_segments),
        )),
    )
}

pub(crate) fn service_operation_key(
    service_schema: &'static ServiceSchema<'static>,
    operation: &'static OperationSchema<'static>,
) -> String {
    format!(
        "{}.{}",
        service_schema.schema().shape_id().shape_name(),
        operation.shape_id().shape_name()
    )
}

impl<S> RouterForOperationHandlerBindings<S> for RestJson1 {
    type Router = RestRouter<S>;

    fn router_for_operation_handler_bindings<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<Self::Router, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        router_for_operation_handler_bindings(service_schema, bindings, "aws.protocols#restJson1", rest_request_spec)
    }
}

impl<S> RouterForOperationHandlerBindings<S> for RestXml {
    type Router = RestRouter<S>;

    fn router_for_operation_handler_bindings<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<Self::Router, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        router_for_operation_handler_bindings(service_schema, bindings, "aws.protocols#restXml", rest_request_spec)
    }
}

impl<S> RouterForOperationHandlerBindings<S> for AwsJson1_0 {
    type Router = AwsJsonRouter<S>;

    fn router_for_operation_handler_bindings<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<Self::Router, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        router_for_operation_handler_bindings(
            service_schema,
            bindings,
            "aws.protocols#awsJson1_0",
            service_operation_key,
        )
    }
}

impl<S> RouterForOperationHandlerBindings<S> for AwsJson1_1 {
    type Router = AwsJsonRouter<S>;

    fn router_for_operation_handler_bindings<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<Self::Router, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        router_for_operation_handler_bindings(
            service_schema,
            bindings,
            "aws.protocols#awsJson1_1",
            service_operation_key,
        )
    }
}

impl<S> RouterForOperationHandlerBindings<S> for RpcV2Cbor {
    type Router = RpcV2CborRouter<S>;

    fn router_for_operation_handler_bindings<I>(
        service_schema: &'static ServiceSchema<'static>,
        bindings: I,
    ) -> Result<Self::Router, BuildError>
    where
        I: IntoIterator<Item = OperationHandlerBinding<S>>,
    {
        router_for_operation_handler_bindings(
            service_schema,
            bindings,
            "smithy.protocols#rpcv2Cbor",
            service_operation_key,
        )
    }
}
