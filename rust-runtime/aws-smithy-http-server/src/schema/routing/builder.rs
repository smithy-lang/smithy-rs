/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Configuration and construction of the multi-protocol routing service.

use super::service::{BoundHandler, ProtocolAndRouter, RoutingState};
use super::{
    MultiProtocolRoutingService, OperationHandlerBinding, OperationTarget, RouterBuildContext, RouterBuildError,
    RoutingOptions, SharedProtocolRouter,
};
use crate::{
    body::BoxBody,
    routing::SyncRoute,
    schema::{
        ProtocolBuildContext, ProtocolOrder, ProtocolRegistration, ProtocolRegistry, ServiceSchema,
        SharedServerProtocol,
    },
};
use http::{Request, Response};
use std::{collections::HashSet, convert::Infallible, sync::Arc};
use tower::{layer::util::Identity, Service};

/// Collects routing inputs, validating and constructing them only in [`Self::build`].
///
/// The service schema is required. Built-in protocols are always available; additional
/// registries contribute protocols implemented outside this crate.
#[derive(Debug)]
pub struct MultiProtocolRoutingServiceBuilder<B = hyper::body::Incoming, L = Identity> {
    service: &'static ServiceSchema<'static>,
    registries: Vec<&'static ProtocolRegistry>,
    bindings: Vec<OperationHandlerBinding<B>>,
    options: RoutingOptions,
    layer: L,
}

impl<B> MultiProtocolRoutingServiceBuilder<B> {
    /// Starts configuring routing for the supplied schema.
    pub fn new(service: &'static ServiceSchema<'static>) -> Self {
        Self {
            service,
            registries: Vec::new(),
            bindings: Vec::new(),
            options: RoutingOptions::default(),
            layer: Identity::new(),
        }
    }

    /// Collects operation bindings and external protocol registries with default options.
    pub fn from_operation_handler_bindings(
        service: &'static ServiceSchema<'static>,
        registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
    ) -> Self {
        Self::new(service)
            .registries(registries)
            .operation_handler_bindings(bindings)
    }

    /// Collects operation bindings, external registries, and routing options.
    pub fn from_operation_handler_bindings_with_options(
        service: &'static ServiceSchema<'static>,
        registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
        options: RoutingOptions,
    ) -> Self {
        Self::from_operation_handler_bindings(service, registries, bindings).options(options)
    }
}

impl<B, L> MultiProtocolRoutingServiceBuilder<B, L> {
    /// Replaces the external registries. [`ProtocolRegistry::BUILTIN`] is always consulted.
    pub fn registries(mut self, registries: impl IntoIterator<Item = &'static ProtocolRegistry>) -> Self {
        self.registries = registries.into_iter().collect();
        self
    }

    /// Replaces the protocol-independent operation bindings.
    pub fn operation_handler_bindings(
        mut self,
        bindings: impl IntoIterator<Item = OperationHandlerBinding<B>>,
    ) -> Self {
        self.bindings = bindings.into_iter().collect();
        self
    }

    /// Replaces the routing options.
    pub fn options(mut self, options: RoutingOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets the handler layer, replacing any previously configured layer.
    ///
    /// The layer is applied during construction. It runs after routing, with the selected
    /// operation in request extensions. Compose multiple layers with a Tower layer stack.
    pub fn layer<N>(self, layer: N) -> MultiProtocolRoutingServiceBuilder<B, N> {
        MultiProtocolRoutingServiceBuilder {
            service: self.service,
            registries: self.registries,
            bindings: self.bindings,
            options: self.options,
            layer,
        }
    }

    /// Validates the configuration and builds protocol routers and layered handlers.
    pub fn build(self) -> Result<MultiProtocolRoutingService<B>, RouterBuildError>
    where
        B: 'static,
        L: tower::Layer<SyncRoute<crate::body::RequestBody<B>>>,
        L::Service: Service<Request<crate::body::RequestBody<B>>, Response = Response<BoxBody>, Error = Infallible>
            + Clone
            + Send
            + Sync
            + 'static,
        <L::Service as Service<Request<crate::body::RequestBody<B>>>>::Future: Send + 'static,
    {
        let Self {
            service,
            registries,
            bindings,
            mut options,
            layer,
        } = self;
        options.request_body = std::mem::take(&mut options.request_body).with_global_settings(
            options
                .protocol_settings
                .get(crate::schema::settings::GLOBAL_SETTINGS_KEY),
        )?;
        let resolved = resolve_protocols(service, registries, &options)?;
        let mut seen = HashSet::new();
        for binding in &bindings {
            let id = binding.operation.shape_id().as_str();
            if !seen.insert(id)
                || !service
                    .operations()
                    .iter()
                    .any(|op| std::ptr::eq(*op, binding.operation))
            {
                return Err(RouterBuildError::Binding(id.to_owned()));
            }
        }
        for operation in service.operations() {
            if !seen.contains(operation.shape_id().as_str()) {
                return Err(RouterBuildError::Binding(format!("missing {}", operation.shape_id())));
            }
        }
        if seen.len() != service.operations().len() {
            return Err(RouterBuildError::Binding("duplicate service operation schemas".into()));
        }
        for id in options.request_body.per_operation.keys() {
            if !seen.contains(id.as_str()) {
                return Err(RouterBuildError::Configuration(format!("unknown operation {id}")));
            }
        }
        let targets: Vec<_> = bindings
            .iter()
            .enumerate()
            .map(|(index, binding)| OperationTarget::new(index, binding.operation))
            .collect();
        // A body-routed protocol may buffer the body to select, so it never sees a streaming
        // operation. Recognized streaming inputs skip claims that require body bytes;
        // claims already made from the head retain their priority.
        let non_streaming: Vec<_> = targets
            .iter()
            .filter(|target| !target.has_streaming_input() && !target.has_streaming_output())
            .copied()
            .collect();
        // `resolved` is already in claim order; build each protocol's router in place. The
        // protocol's kind picks its target set inside `build_router`: a body-routed protocol
        // gets only the non-streaming operations.
        let mut protocols = Vec::with_capacity(resolved.len());
        for protocol in resolved {
            let router = protocol.build_router(
                RouterBuildContext {
                    service,
                    targets: &targets,
                    config: &options.request_body,
                    protocol_settings: options.protocol_settings.get(protocol.protocol_id().as_str()),
                },
                &non_streaming,
            )?;
            protocols.push(ProtocolAndRouter { router, protocol });
        }
        // Whether recognition is needed comes from the service schema. Each metadata
        // router owns recognition of the streaming operations its protocol supports.
        let has_streaming_inputs = targets.iter().any(|target| target.has_streaming_input());
        let metadata_routers = has_streaming_inputs.then(|| {
            protocols
                .iter()
                .enumerate()
                .filter_map(|(index, route)| matches!(route.router, SharedProtocolRouter::Metadata(_)).then_some(index))
                .collect::<Box<[_]>>()
        });
        let bindings = bindings
            .into_iter()
            .map(|binding| BoundHandler {
                operation: binding.operation,
                collection_config: options.request_body.for_operation(binding.operation.shape_id()),
                handler_route: SyncRoute::new(layer.layer(binding.route)),
            })
            .collect();
        Ok(MultiProtocolRoutingService {
            state: Arc::new(RoutingState {
                protocols: protocols.into(),
                metadata_routers,
                handlers: bindings,
                body_collection_config: options.request_body.for_routing(),
            }),
        })
    }
}

/// Resolves the registered protocols the service declares, in claim order.
///
/// The order comes from [`ProtocolOrder`] constraints alone, resolved over the **global** set of
/// registered protocols: a constraint against a protocol the service does not serve still orders
/// the ones it does, transitively. Registry and declaration order carry no meaning. Errors:
/// a declared protocol without a registration, a protocol registered twice, a constraint naming
/// an unregistered protocol, a constraint cycle, or two served protocols left unordered.
fn resolve_protocols(
    service: &'static ServiceSchema<'static>,
    registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
    options: &RoutingOptions,
) -> Result<Vec<SharedServerProtocol>, RouterBuildError> {
    let mut registrations: Vec<ProtocolRegistration> = Vec::new();
    registrations.extend_from_slice(ProtocolRegistry::BUILTIN.registrations());
    for registry in registries {
        registrations.extend_from_slice(registry.registrations());
    }
    for (index, registration) in registrations.iter().enumerate() {
        if registrations[..index]
            .iter()
            .any(|other| other.protocol_id() == registration.protocol_id())
        {
            return Err(RouterBuildError::DuplicateProtocol {
                protocol: registration.protocol_id().to_string(),
            });
        }
    }

    // Validate every declaration before invoking any protocol factory or building its router.
    let missing: Vec<String> = service
        .protocols()
        .iter()
        .filter(|protocol| {
            !registrations
                .iter()
                .any(|registration| registration.protocol_id() == protocol.as_str())
        })
        .map(|protocol| protocol.to_string())
        .collect();
    if !missing.is_empty() {
        return Err(RouterBuildError::MissingProtocols { protocols: missing });
    }

    // Reachability over the global constraint graph, absent protocols included as transit nodes.
    let count = registrations.len();
    let position = |id: &str| {
        registrations
            .iter()
            .position(|registration| registration.protocol_id() == id)
    };
    let mut reaches = vec![vec![false; count]; count];
    for (index, registration) in registrations.iter().enumerate() {
        for constraint in registration.order() {
            let id = match *constraint {
                ProtocolOrder::Before(id) | ProtocolOrder::After(id) => id,
            };
            let other = position(id).ok_or_else(|| {
                RouterBuildError::Configuration(format!(
                    "protocol {} orders against unregistered protocol {id}",
                    registration.protocol_id()
                ))
            })?;
            match *constraint {
                ProtocolOrder::Before(_) => reaches[index][other] = true,
                ProtocolOrder::After(_) => reaches[other][index] = true,
            }
        }
    }
    for via in 0..count {
        for from in 0..count {
            if reaches[from][via] {
                for to in 0..count {
                    if reaches[via][to] {
                        reaches[from][to] = true;
                    }
                }
            }
        }
    }
    let cyclic_protocols: Vec<String> = (0..count)
        .filter(|&index| reaches[index][index])
        .map(|index| registrations[index].protocol_id().to_string())
        .collect();
    if !cyclic_protocols.is_empty() {
        return Err(RouterBuildError::ProtocolOrderCycle {
            protocols: cyclic_protocols,
        });
    }

    let mut served: Vec<usize> = registrations
        .iter()
        .enumerate()
        .filter(|(_, registration)| {
            service
                .protocols()
                .iter()
                .any(|protocol| protocol.as_str() == registration.protocol_id())
        })
        .map(|(index, _)| index)
        .collect();
    if served.is_empty() {
        return Err(RouterBuildError::UnknownProtocol);
    }
    for (nth, &first) in served.iter().enumerate() {
        for &second in &served[nth + 1..] {
            if !reaches[first][second] && !reaches[second][first] {
                return Err(RouterBuildError::AmbiguousProtocolOrder {
                    first: registrations[first].protocol_id().to_string(),
                    second: registrations[second].protocol_id().to_string(),
                });
            }
        }
    }
    // Every served pair is comparable and the graph is acyclic, so reachability totally orders them.
    served.sort_by(|&first, &second| {
        if reaches[first][second] {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });

    served
        .into_iter()
        .map(|index| {
            let registration = &registrations[index];
            let context = ProtocolBuildContext::new(service)
                .with_settings(options.protocol_settings.get(registration.protocol_id()))
                .with_global(
                    options
                        .protocol_settings
                        .get(crate::schema::settings::GLOBAL_SETTINGS_KEY),
                );
            registration.build(&context)
        })
        .collect()
}
