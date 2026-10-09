/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

//! Configuration and construction of the multi-protocol routing service.

use super::service::{BoundHandler, ProtocolAndRouter, RoutingState};
use super::{
    MultiProtocolRoutingService, OperationTarget, ProtocolOptions, ProtocolResolutionError, RouterBuildContext,
    RouterBuildError, SharedProtocolRouter,
};
use crate::{
    body::BoxBody,
    routing::SyncRoute,
    schema::{
        OperationSchema, ProtocolBuildContext, ProtocolOrder, ProtocolRegistration, ProtocolRegistry, ServiceSchema,
        SharedServerProtocol,
    },
};
use http::{Request, Response};
use std::collections::HashMap;
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
    bindings: Vec<(
        &'static OperationSchema<'static>,
        SyncRoute<crate::body::RequestBody<B>>,
    )>,
    options: ProtocolOptions,
    layer: L,
}

impl<B> MultiProtocolRoutingServiceBuilder<B> {
    /// Starts configuring routing for the supplied schema.
    pub fn new(service: &'static ServiceSchema<'static>) -> Self {
        Self {
            service,
            registries: Vec::new(),
            bindings: Vec::new(),
            options: ProtocolOptions::default(),
            layer: Identity::new(),
        }
    }

    /// Collects operation bindings and external protocol registries with default options.
    pub fn from_operation_handler_bindings(
        service: &'static ServiceSchema<'static>,
        registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
        bindings: impl IntoIterator<
            Item = (
                &'static OperationSchema<'static>,
                SyncRoute<crate::body::RequestBody<B>>,
            ),
        >,
    ) -> Self {
        Self::new(service)
            .registries(registries)
            .operation_handler_bindings(bindings)
    }

    /// Collects operation bindings, external registries, and protocol options.
    pub fn from_operation_handler_bindings_with_options(
        service: &'static ServiceSchema<'static>,
        registries: impl IntoIterator<Item = &'static ProtocolRegistry>,
        bindings: impl IntoIterator<
            Item = (
                &'static OperationSchema<'static>,
                SyncRoute<crate::body::RequestBody<B>>,
            ),
        >,
        options: ProtocolOptions,
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

    /// Replaces the protocol-independent `(operation schema, handler route)` bindings.
    pub fn operation_handler_bindings(
        mut self,
        bindings: impl IntoIterator<
            Item = (
                &'static OperationSchema<'static>,
                SyncRoute<crate::body::RequestBody<B>>,
            ),
        >,
    ) -> Self {
        self.bindings = bindings.into_iter().collect();
        self
    }

    /// Replaces the protocol options.
    pub fn options(mut self, options: ProtocolOptions) -> Self {
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
            options,
            layer,
        } = self;
        let resolved = resolve_protocols(service, registries, &options)?;

        // The operaiton must be listed in the Service shape.
        let mut seen = HashSet::new();
        for (operation, _) in &bindings {
            let id = operation.shape_id().as_str();
            if !seen.insert(id) || !service.operations().iter().any(|op| std::ptr::eq(*op, *operation)) {
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
        for id in options.service_config.request_body.per_operation.keys() {
            if !seen.contains(id.as_str()) {
                return Err(RouterBuildError::Configuration(format!("unknown operation {id}")));
            }
        }
        let targets: Vec<_> = bindings
            .iter()
            .enumerate()
            .map(|(index, (operation, _))| OperationTarget::new(index, operation))
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
                    config: &options.service_config,
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
            .map(|(operation, route)| BoundHandler {
                operation,
                collection_config: options.service_config.request_body.for_operation(operation.shape_id()),
                handler_route: SyncRoute::new(layer.layer(route)),
            })
            .collect();
        Ok(MultiProtocolRoutingService {
            state: Arc::new(RoutingState {
                protocols: protocols.into(),
                metadata_routers,
                handlers: bindings,
                body_collection_config: options.service_config.request_body.for_routing(),
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
    options: &ProtocolOptions,
) -> Result<Vec<SharedServerProtocol>, RouterBuildError> {
    let mut registrations = Vec::new();
    let mut positions: HashMap<&str, usize> = HashMap::new();

    let registered_protocols = ProtocolRegistry::BUILTIN.registrations().iter().chain(
        registries
            .into_iter()
            .flat_map(|registry| registry.registrations().iter()),
    );

    for p in registered_protocols {
        if positions.insert(p.protocol_id(), registrations.len()).is_some() {
            return Err(ProtocolResolutionError::DuplicateRegistration {
                protocol: p.protocol_id().to_string(),
            }
            .into());
        }

        registrations.push(p);
    }

    // Validate all declared protocols are registered.
    let missing: Vec<String> = service
        .protocols()
        .iter()
        .filter(|protocol| !positions.contains_key(protocol.as_str()))
        .map(|protocol| protocol.to_string())
        .collect();
    if !missing.is_empty() {
        return Err(ProtocolResolutionError::MissingRegistrations { protocols: missing }.into());
    }

    // Reachability over the global constraint graph, absent protocols included as transit nodes.

    let protocol_position = |id: &str, registration: &ProtocolRegistration| {
        positions.get(id).copied().ok_or_else(|| {
            RouterBuildError::Configuration(format!(
                "protocol {} orders against unregistered protocol {id}",
                registration.protocol_id()
            ))
        })
    };

    let count = registrations.len();

    let mut reaches = vec![vec![false; count]; count];
    for (index, registration) in registrations.iter().enumerate() {
        for constraint in registration.order() {
            let (from, to) = match *constraint {
                ProtocolOrder::Before(id) => (index, protocol_position(id, registration)?),
                ProtocolOrder::After(id) => (protocol_position(id, registration)?, index),
            };

            reaches[from][to] = true
        }
    }

    // Set transitive edges:
    // A -> B
    // B -> C
    // A -> C
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
        return Err(ProtocolResolutionError::OrderCycle {
            protocols: cyclic_protocols,
        }
        .into());
    }

    let mut served = service_protocols(service, &registrations);
    if served.is_empty() {
        return Err(ProtocolResolutionError::NoProtocolsDeclared.into());
    }

    for (nth, &first) in served.iter().enumerate() {
        for &second in &served[nth + 1..] {
            if !reaches[first][second] && !reaches[second][first] {
                return Err(ProtocolResolutionError::AmbiguousOrder {
                    first: registrations[first].protocol_id().to_string(),
                    second: registrations[second].protocol_id().to_string(),
                }
                .into());
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
                .with_config(&options.service_config)
                .with_settings(options.protocol_settings.get(registration.protocol_id()));
            registration.build(&context)
        })
        .collect()
}

/// Returns protocol indices that the service uses.
fn service_protocols(
    service: &'static ServiceSchema<'static>,
    registrations: &Vec<&ProtocolRegistration>,
) -> Vec<usize> {
    registrations
        .iter()
        .enumerate()
        .filter(|(_, registration)| {
            service
                .protocols()
                .iter()
                .any(|protocol| protocol.as_str() == registration.protocol_id())
        })
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::*;
    use super::super::MultiProtocolRoutingService;
    use super::*;
    use crate::body::Body;
    use crate::response::Response;
    use crate::schema::SelectedOperation;
    use aws_smithy_schema::shape_id;
    use bytes::Bytes;
    use http::HeaderValue;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    #[test]
    fn binding_and_protocol_validation() {
        // SERVICE declares FIRST and SECOND; omitting SECOND's handler must fail the build.
        assert!(matches!(
            MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
                &SERVICE,
                [registry()],
                [binding(&FIRST)]
            )
            .build(),
            Err(RouterBuildError::Binding(_))
        ));
        // Registering FIRST's handler twice must fail the build.
        assert!(matches!(
            MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
                &SERVICE,
                [registry()],
                [binding(&FIRST), binding(&FIRST)]
            )
            .build(),
            Err(RouterBuildError::Binding(_))
        ));
        // Both handlers are present, but SERVICE's custom protocol has no registration.
        assert!(matches!(
            MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(&SERVICE, [], [binding(&FIRST), binding(&SECOND)]).build(),
            Err(RouterBuildError::ProtocolResolution(ProtocolResolutionError::MissingRegistrations { protocols })) if protocols == ["test#bodyRouting"]
        ));
        static NONE: ServiceSchema<'static> = ServiceSchema::new(SERVICE_ID, None, &[], OPERATIONS);
        static MANY: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("aws.protocols", "restXml"),
            ],
            OPERATIONS,
        );
        // A service must declare at least one protocol, even when all handlers are present.
        assert!(matches!(
            MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
                &NONE,
                [],
                [binding(&FIRST), binding(&SECOND)]
            )
            .build(),
            Err(RouterBuildError::ProtocolResolution(
                ProtocolResolutionError::NoProtocolsDeclared
            ))
        ));
        // Both declared built-in protocols must be included in the constructed service.
        let many = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &MANY,
            [],
            [binding(&FIRST), binding(&SECOND)],
        )
        .build()
        .expect("every declared protocol is served");
        assert_eq!(many.state.protocols.len(), 2);
        // Matching FIRST's shape ID is insufficient: the binding must reference the
        // exact operation schema declared by SERVICE.
        static COPY: OperationSchema<'static> = OperationSchema::new(FIRST_ID, &FIRST_INPUT, &UNIT, &[]);
        assert!(matches!(
            MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
                &SERVICE,
                [registry()],
                [binding(&COPY), binding(&SECOND)]
            )
            .build(),
            Err(RouterBuildError::Binding(_))
        ));
    }

    #[test]
    fn partially_registered_service_reports_every_missing_protocol_before_building() {
        static PARTIAL: ServiceSchema<'static> = ServiceSchema::new(
            SERVICE_ID,
            None,
            &[
                shape_id!("aws.protocols", "restJson1"),
                shape_id!("test", "unregisteredFirst"),
                shape_id!("test", "unregisteredSecond"),
            ],
            OPERATIONS,
        );
        // Missing registrations fail even before the missing operation bindings are checked.
        let error = MultiProtocolRoutingServiceBuilder::<Body>::from_operation_handler_bindings(&PARTIAL, [], [])
            .build()
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "missing protocol registrations: test#unregisteredFirst, test#unregisteredSecond"
        );
        let source = std::error::Error::source(&error).expect("protocol resolution retains its typed error source");
        assert!(matches!(
            source.downcast_ref::<ProtocolResolutionError>(),
            Some(ProtocolResolutionError::MissingRegistrations { protocols })
                if protocols == &["test#unregisteredFirst", "test#unregisteredSecond"]
        ));
        assert!(matches!(
            error,
            RouterBuildError::ProtocolResolution(ProtocolResolutionError::MissingRegistrations { protocols })
                if protocols == ["test#unregisteredFirst", "test#unregisteredSecond"]
        ));
    }

    #[test]
    fn invalid_protocol_settings_fail_the_build() {
        for settings in [
            r#"{"capitalizeRoutes":"yes"}"#,
            r#"{"methodNotAllowedAsNotFound":"yes"}"#,
            r#"{"methodNotAllowedAsNotFound":null}"#,
            r#""not an object""#,
        ] {
            let options = ProtocolOptions {
                protocol_settings: HashMap::from([(
                    shape_id!("smithy.protocols", "rpcv2Cbor"),
                    crate::schema::settings::parse_settings_json(settings.as_bytes()),
                )]),
                ..Default::default()
            };
            assert!(matches!(
                MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings_with_options(
                    &RPC,
                    [],
                    [binding(&SECOND), binding(&FIRST)],
                    options,
                )
                .build(),
                Err(RouterBuildError::Configuration(_))
            ));
        }
    }

    #[test]
    fn builtins_follow_their_chained_constraints_whatever_the_declaration_order() {
        assert_eq!(
            priority(&app(&BUILTINS, [])),
            [
                "smithy.protocols#rpcv2Cbor",
                "aws.protocols#awsJson1_0",
                "aws.protocols#awsJson1_1",
                "aws.protocols#restJson1",
                "aws.protocols#restXml",
            ]
        );
    }

    #[test]
    fn unordered_served_protocols_fail_the_build() {
        let error = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(&[])],
            OPS.iter().map(|operation| echo(operation)),
        )
        .build()
        .unwrap_err();
        assert!(
            matches!(
                error,
                RouterBuildError::ProtocolResolution(ProtocolResolutionError::AmbiguousOrder { .. })
            ),
            "{error}"
        );
    }

    #[test]
    fn a_duplicate_protocol_fails_the_build() {
        let error = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(&[]), body_routing(&[])],
            OPS.iter().map(|operation| echo(operation)),
        )
        .build()
        .unwrap_err();
        assert!(
            matches!(error, RouterBuildError::ProtocolResolution(ProtocolResolutionError::DuplicateRegistration { protocol }) if protocol == "test#bodyRouting"),
        );
    }

    #[test]
    fn a_constraint_against_an_unregistered_protocol_fails_the_build() {
        static TYPO: &[ProtocolOrder] = &[ProtocolOrder::Before("aws.protocols#restJson2")];
        let error = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(TYPO)],
            OPS.iter().map(|operation| echo(operation)),
        )
        .build()
        .unwrap_err();
        assert!(matches!(error, RouterBuildError::Configuration(_)), "{error}");
    }

    #[test]
    fn contradictory_ordering_fails_the_build() {
        static CYCLE: &[ProtocolOrder] = &[
            ProtocolOrder::Before("aws.protocols#restJson1"),
            ProtocolOrder::After("aws.protocols#restJson1"),
        ];
        static ABSENT: &[ProtocolOrder] = &[ProtocolOrder::After("aws.protocols#restXml")];
        let error = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(CYCLE)],
            OPS.iter().map(|operation| echo(operation)),
        )
        .build()
        .unwrap_err();
        assert_eq!(
            error.to_string(),
            "protocol ordering constraints form a cycle involving: aws.protocols#restJson1, test#bodyRouting"
        );
        assert!(matches!(
            error,
            RouterBuildError::ProtocolResolution(ProtocolResolutionError::OrderCycle { protocols })
                if protocols == ["aws.protocols#restJson1", "test#bodyRouting"]
        ));
        // A constraint against an unserved protocol still orders the served ones transitively:
        // restJson1 comes before restXml (builtin chain) and restXml before bodyRouting, so
        // restJson1 precedes bodyRouting even though restXml is not served.
        assert_eq!(
            priority(&app(&WITH_BODY_ROUTING, [body_routing(ABSENT)])),
            ["aws.protocols#restJson1", "test#bodyRouting"]
        );
    }

    #[test]
    fn ordering_cycle_reports_transitive_members_including_unserved_protocols() {
        static CYCLE: &[ProtocolOrder] = &[
            ProtocolOrder::Before("aws.protocols#restJson1"),
            ProtocolOrder::After("aws.protocols#restXml"),
        ];
        let error = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(CYCLE)],
            OPS.iter().map(|operation| echo(operation)),
        )
        .build()
        .unwrap_err();
        assert!(matches!(
            error,
            RouterBuildError::ProtocolResolution(ProtocolResolutionError::OrderCycle { protocols })
                if protocols == ["aws.protocols#restJson1", "aws.protocols#restXml", "test#bodyRouting"]
        ));
    }

    #[test]
    fn self_ordering_cycle_reports_only_the_cyclic_protocol() {
        static CYCLE: &[ProtocolOrder] = &[ProtocolOrder::Before("test#bodyRouting")];
        let error = MultiProtocolRoutingServiceBuilder::from_operation_handler_bindings(
            &WITH_BODY_ROUTING,
            [body_routing(CYCLE)],
            OPS.iter().map(|operation| echo(operation)),
        )
        .build()
        .unwrap_err();
        assert!(matches!(
            error,
            RouterBuildError::ProtocolResolution(ProtocolResolutionError::OrderCycle { protocols }) if protocols == ["test#bodyRouting"]
        ));
    }

    #[test]
    fn builder_allows_configuration_to_be_replaced_before_validation() {
        let builder = MultiProtocolRoutingService::builder(&SERVICE)
            .registries([registry(), registry()])
            .operation_handler_bindings([binding(&FIRST), binding(&FIRST)])
            .options(
                ProtocolOptions::default().with_request_body(crate::schema::ServiceRequestBodyConfig {
                    per_operation: [("test#unknown".to_owned(), config(1, 1))].into(),
                    ..Default::default()
                }),
            );
        let app = builder
            .registries([registry()])
            .operation_handler_bindings([binding(&FIRST), binding(&SECOND)])
            .options(ProtocolOptions::default())
            .build();
        assert!(app.is_ok());
        let invalid = MultiProtocolRoutingService::<hyper::body::Incoming>::builder(&REST_JSON);
        assert!(matches!(invalid.build(), Err(RouterBuildError::Binding(_))));
    }

    #[tokio::test]
    async fn builder_supports_custom_transport_bodies_with_handler_layers() {
        type Transport = http_body_util::Full<Bytes>;
        for schema in [&REST_JSON, &SERVICE] {
            let bindings = schema.operations().iter().map(|operation| {
                (
                    *operation,
                    SyncRoute::new(tower::service_fn(
                        |request: Request<crate::body::RequestBody<Transport>>| async move {
                            assert!(request.extensions().get::<SelectedOperation>().is_some());
                            Ok::<_, Infallible>(Response::new(crate::body::boxed(request.into_body())))
                        },
                    )),
                )
            });
            let app = MultiProtocolRoutingService::<Transport>::builder(schema)
                .registries([registry()])
                .operation_handler_bindings(bindings)
                .layer(tower::util::MapResponseLayer::new(|mut response: Response<BoxBody>| {
                    response
                        .headers_mut()
                        .insert("x-layer", HeaderValue::from_static("applied"));
                    response
                }))
                .build()
                .unwrap();
            let response = app
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/first")
                        .header("content-type", "application/json")
                        .body(Transport::new(Bytes::from_static(b"first\npayload")))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.headers()["x-layer"], "applied");
            assert_eq!(
                response.into_body().collect().await.unwrap().to_bytes(),
                "first\npayload"
            );
        }
    }
}
