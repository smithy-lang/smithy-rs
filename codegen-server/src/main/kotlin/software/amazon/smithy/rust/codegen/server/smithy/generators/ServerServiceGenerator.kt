/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.PatternTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Error as ErrorModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Input as InputModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule.Output as OutputModule

class ServerServiceGenerator(
    private val codegenContext: ServerCodegenContext,
    private val protocol: ServerProtocol,
    private val isConfigBuilderFallible: Boolean,
    private val selectedProtocols: List<ServerProtocol>,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
    private val codegenScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "Http" to RuntimeType.http(runtimeConfig),
            "SmithyHttp" to RuntimeType.smithyHttp(runtimeConfig),
            "HttpBody" to RuntimeType.httpBody(runtimeConfig),
            "SmithyHttpServer" to smithyHttpServer,
            "Tower" to RuntimeType.Tower,
            *RuntimeType.preludeScope,
        )
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val crateName = codegenContext.moduleUseName()

    private val service = codegenContext.serviceShape
    private val serviceId = service.id
    private val serviceName = serviceId.name.toPascalCase()
    private val builderName = "${serviceName}Builder"
    private val routerName = "${serviceName}Router"

    private val isMultiProtocol = selectedProtocols.size > 1

    /** Calculate all `operationShape`s contained within the `ServiceShape`. */
    private val index = TopDownIndex.of(codegenContext.model)
    private val operations = index.getContainedOperations(codegenContext.serviceShape).toSortedSet(compareBy { it.id })

    /** Associate each operation with the corresponding field names in the builder struct. */
    private val builderFieldNames =
        operations.associateWith { RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(it).name.toSnakeCase()) }
            .toSortedMap(
                compareBy { it.id },
            )

    /** Associate each operation with the name of the corresponding Zero-Sized Type (ZST) struct name. */
    private val operationStructNames = operations.associateWith { symbolProvider.toSymbol(it).name.toPascalCase() }

    private val protocolRoutesName = "${serviceName}ProtocolRoutes"

    /** A `Writable` block of "field: Type" for the builder. */
    private val builderFields =
        builderFieldNames.values.map { name ->
            if (isMultiProtocol) {
                "$name: Option<$protocolRoutesName<Body>>"
            } else {
                "$name: Option<#{SmithyHttpServer}::routing::Route<Body>>"
            }
        }

    /** The name of the local private module containing the functions that return the request for each operation */
    private val requestSpecsModuleName = "request_specs"

    private data class RequestSpecInfo(
        val functionName: String,
        val functionBody: Writable,
    )

    /** Associate each operation with a function that returns its request spec. */
    private val requestSpecMap: Map<OperationShape, RequestSpecInfo> =
        operations.associateWith { operationShape ->
            val operationName = symbolProvider.toSymbol(operationShape).name
            val spec =
                protocol.serverRouterRequestSpec(
                    operationShape,
                    operationName,
                    serviceId.name,
                    smithyHttpServer.resolve("routing::request_spec"),
                )
            val functionName = RustReservedWords.escapeIfNeeded(operationName.toSnakeCase())
            val functionBody =
                writable {
                    rustTemplate(
                        """
                        fn $functionName() -> #{SpecType} {
                            #{Spec:W}
                        }
                        """,
                        "Spec" to spec,
                        "SpecType" to protocol.serverRouterRequestSpecType(smithyHttpServer.resolve("routing::request_spec")),
                    )
                }
            RequestSpecInfo(functionName, functionBody)
        }

    private fun protocolIdentifier(
        protocol: ServerProtocol,
        index: Int,
    ): String = "p${index}_${protocol.protocolShapeId.name.toSnakeCase()}"

    /** Request specifications grouped by generated protocol identifier and operation. */
    private val multiProtocolRequestSpecMap: Map<String, Map<OperationShape, RequestSpecInfo>> by lazy {
        if (!isMultiProtocol) {
            emptyMap()
        } else {
            selectedProtocols.mapIndexed { index, proto ->
                val identifier = protocolIdentifier(proto, index)
                val specsMap =
                    operations.associateWith { operationShape ->
                        val operationName = symbolProvider.toSymbol(operationShape).name
                        val spec =
                            proto.serverRouterRequestSpec(
                                operationShape,
                                operationName,
                                serviceId.name,
                                smithyHttpServer.resolve("routing::request_spec"),
                            )
                        // Append the protocol identifier to avoid function-name collisions.
                        val functionName =
                            RustReservedWords.escapeIfNeeded(operationName.toSnakeCase()) + "_" + identifier
                        val functionBody =
                            writable {
                                rustTemplate(
                                    """
                                    fn $functionName() -> #{SpecType} {
                                        #{Spec:W}
                                    }
                                    """,
                                    "Spec" to spec,
                                    "SpecType" to proto.serverRouterRequestSpecType(smithyHttpServer.resolve("routing::request_spec")),
                                )
                            }
                        RequestSpecInfo(functionName, functionBody)
                    }
                identifier to specsMap
            }.toMap()
        }
    }

    /** Router metadata for each selected protocol, in detection order. */
    private val protocolInfos: List<ProtocolRouterInfo> by lazy {
        if (!isMultiProtocol) {
            emptyList()
        } else {
            selectedProtocols.mapIndexed { index, proto ->
                ProtocolRouterInfo(
                    identifier = protocolIdentifier(proto, index),
                    markerStruct = proto.markerStruct(),
                    routerType = proto.routerType(),
                )
            }
        }
    }

    /** Helper data class for protocol router info. */
    private data class ProtocolRouterInfo(
        val identifier: String,
        val markerStruct: RuntimeType,
        val routerType: RuntimeType,
    )

    /**
     * Returns the marker and router type arguments for
     * `ProtocolService<Marker, Router<S>, Inner>`.
     */
    private fun ProtocolRouterInfo.protocolServiceTypeParams(): Writable =
        writable {
            rustTemplate(
                "#{Marker}, #{Router}<S>",
                "Router" to routerType,
                "Marker" to markerStruct,
            )
        }

    /** Applies the configured route layer to each protocol router. */
    private fun routingServiceConstructions(protocolInfos: List<ProtocolRouterInfo>): Writable =
        writable {
            for (protoInfo in protocolInfos) {
                val routerVarName = "${protoInfo.identifier}_router"
                rustTemplate(
                    """
                    let $routerVarName = $routerVarName.layer(&self.layer);
                    """,
                    *codegenScope,
                )
            }
        }

    /**
     * Generates the service router alias.
     *
     * Multi-protocol aliases nest `ProtocolService<Marker, Router<S>, Inner>` and
     * terminate in `DefaultNotFoundService`; single-protocol aliases wrap `RoutingService`.
     */
    private fun routerTypeAlias(): Writable =
        writable {
            if (isMultiProtocol) {
                // Build nested `ProtocolService<Marker, Router<S>, Inner>` types in canonical
                // detection order, with the first protocol outermost.
                val serviceType =
                    writable {
                        for (protoInfo in protocolInfos) {
                            rustTemplate(
                                "#{SmithyHttpServer}::routing::ProtocolService<#{Params:W}, ",
                                *codegenScope,
                                "Params" to protoInfo.protocolServiceTypeParams(),
                            )
                        }
                        rustTemplate(
                            "#{SmithyHttpServer}::routing::DefaultNotFoundService",
                            *codegenScope,
                        )
                        // Close one nested ProtocolService type per protocol.
                        rust(protocolInfos.joinToString("") { ">" })
                    }

                rustTemplate(
                    """
                    /// Type alias for the multi-protocol router used by this service.
                    ///
                    /// Protocol detectors are evaluated in the service's configured detection order.
                    /// A matching detector dispatches through its associated router.
                    ///
                    /// `S` is the service type stored by each protocol router and defaults to `Route`.
                    pub type $routerName<S = #{SmithyHttpServer}::routing::Route> = #{ServiceType:W};
                    """,
                    *codegenScope,
                    "ServiceType" to serviceType,
                )
            }
        }

    /** A `Writable` block containing all the `Handler` and `Operation` setters for the builder. */
    private fun protocolRoutes(): Writable =
        writable {
            if (isMultiProtocol) {
                val fields =
                    writable {
                        for (protoInfo in protocolInfos) {
                            rustTemplate(
                                "${protoInfo.identifier}: #{SmithyHttpServer}::routing::Route<Body>,",
                                *codegenScope,
                            )
                        }
                    }
                rustTemplate(
                    """
                    struct $protocolRoutesName<Body> {
                        #{Fields:W}
                    }
                    """,
                    "Fields" to fields,
                )
            }
        }

    private fun protocolUpgradeBounds(operationType: String): Writable =
        writable {
            if (!isMultiProtocol) {
                rustTemplate(
                    """
                    #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$operationType,
                            ModelPl::Output
                        >,
                        HttpPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$operationType,
                            <
                                #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>
                                as #{SmithyHttpServer}::plugin::Plugin<
                                    $serviceName<L>,
                                    crate::operation_shape::$operationType,
                                    ModelPl::Output
                                >
                            >::Output
                        >,

                        HttpPl::Output: #{Tower}::Service<#{Http}::Request<Body>, Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>, Error = ::std::convert::Infallible> + Clone + Send + 'static,
                        <HttpPl::Output as #{Tower}::Service<#{Http}::Request<Body>>>::Future: Send + 'static,
                    """,
                    *codegenScope,
                )
            } else {
                rust("ModelPl::Output: Clone,")
                for (protoInfo in protocolInfos) {
                    rustTemplate(
                        """
                        #{SmithyHttpServer}::operation::ProtocolUpgradePlugin::<#{Protocol}, UpgradeExtractors>: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$operationType,
                            ModelPl::Output
                        >,
                        HttpPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$operationType,
                            <
                                #{SmithyHttpServer}::operation::ProtocolUpgradePlugin::<#{Protocol}, UpgradeExtractors>
                                as #{SmithyHttpServer}::plugin::Plugin<
                                    $serviceName<L>,
                                    crate::operation_shape::$operationType,
                                    ModelPl::Output
                                >
                            >::Output
                        >,
                        <
                            HttpPl as #{SmithyHttpServer}::plugin::Plugin<
                                $serviceName<L>,
                                crate::operation_shape::$operationType,
                                <
                                    #{SmithyHttpServer}::operation::ProtocolUpgradePlugin::<#{Protocol}, UpgradeExtractors>
                                    as #{SmithyHttpServer}::plugin::Plugin<
                                        $serviceName<L>,
                                        crate::operation_shape::$operationType,
                                        ModelPl::Output
                                    >
                                >::Output
                            >
                        >::Output: #{Tower}::Service<#{Http}::Request<Body>, Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>, Error = ::std::convert::Infallible> + Clone + Send + 'static,
                        <<
                            HttpPl as #{SmithyHttpServer}::plugin::Plugin<
                                $serviceName<L>,
                                crate::operation_shape::$operationType,
                                <
                                    #{SmithyHttpServer}::operation::ProtocolUpgradePlugin::<#{Protocol}, UpgradeExtractors>
                                    as #{SmithyHttpServer}::plugin::Plugin<
                                        $serviceName<L>,
                                        crate::operation_shape::$operationType,
                                        ModelPl::Output
                                    >
                                >::Output
                            >
                        >::Output as #{Tower}::Service<#{Http}::Request<Body>>>::Future: Send + 'static,
                        """,
                        *codegenScope,
                        "Protocol" to protoInfo.markerStruct,
                    )
                }
            }
        }

    private fun protocolRouteConstruction(fieldName: String): Writable =
        writable {
            if (!isMultiProtocol) {
                rustTemplate(
                    """
                    let svc = #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>::new().apply(svc);
                    let svc = self.http_plugin.apply(svc);
                    self.${fieldName}_custom(svc)
                    """,
                    *codegenScope,
                )
            } else {
                for ((index, protoInfo) in protocolInfos.withIndex()) {
                    val modeledService = if (index == protocolInfos.lastIndex) "svc" else "svc.clone()"
                    val protocolService = "${protoInfo.identifier}_svc"
                    val protocolRoute = "${protoInfo.identifier}_route"
                    rustTemplate(
                        """
                        let $protocolService = #{SmithyHttpServer}::operation::ProtocolUpgradePlugin::<#{Protocol}, UpgradeExtractors>::new().apply($modeledService);
                        let $protocolService = self.http_plugin.apply($protocolService);
                        let $protocolRoute = #{SmithyHttpServer}::routing::Route::new($protocolService);
                        """,
                        *codegenScope,
                        "Protocol" to protoInfo.markerStruct,
                    )
                }
                rust("self.$fieldName = Some($protocolRoutesName {")
                for (protoInfo in protocolInfos) {
                    rust("${protoInfo.identifier}: ${protoInfo.identifier}_route,")
                }
                rust("});")
                rust("self")
            }
        }

    private fun customSetter(
        fieldName: String,
        structName: String,
    ): Writable =
        writable {
            if (!isMultiProtocol) {
                rustTemplate(
                    """
                    /// Sets the [`$structName`](crate::operation_shape::$structName) to a custom [`Service`](tower::Service).
                    /// not constrained by the Smithy contract.
                    fn ${fieldName}_custom<S>(mut self, svc: S) -> Self
                    where
                        S: #{Tower}::Service<#{Http}::Request<Body>, Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>, Error = ::std::convert::Infallible> + Clone + Send + 'static,
                        S::Future: Send + 'static,
                    {
                        self.$fieldName = Some(#{SmithyHttpServer}::routing::Route::new(svc));
                        self
                    }
                    """,
                    *codegenScope,
                )
            }
        }

    private fun builderSetters(): Writable =
        writable {
            for ((operationShape, structName) in operationStructNames) {
                val fieldName = builderFieldNames.getValue(operationShape)
                val docHandler = DocHandlerGenerator(codegenContext, operationShape, "handler", "///")
                val handler = docHandler.docSignature()
                val handlerFixed = docHandler.docFixedSignature()
                val selfReceiver = if (isMultiProtocol) "mut self" else "self"
                val routerImport = if (isMultiProtocol) ", $routerName" else ""
                val serviceTypeAssertion =
                    if (isMultiProtocol) {
                        "$serviceName<$routerName>"
                    } else {
                        "$serviceName<#{SmithyHttpServer}::routing::RoutingService<#{Router}<#{SmithyHttpServer}::routing::Route>, #{Protocol}>>"
                    }
                val unwrapConfigBuilder =
                    if (isConfigBuilderFallible) {
                        ".expect(\"config failed to build\")"
                    } else {
                        ""
                    }
                rustTemplate(
                    """
                    /// Sets the [`$structName`](crate::operation_shape::$structName) operation.
                    ///
                    /// This should be an async function satisfying the [`Handler`](#{SmithyHttpServer}::operation::Handler) trait.
                    /// See the [operation module documentation](#{SmithyHttpServer}::operation) for more information.
                    ///
                    /// ## Example
                    ///
                    /// ```no_run
                    /// use $crateName::{$serviceName, ${serviceName}Config$routerImport};
                    ///
                    #{HandlerImports:W}
                    ///
                    #{Handler:W}
                    ///
                    /// let config = ${serviceName}Config::builder().build()$unwrapConfigBuilder;
                    /// let app = $serviceName::builder(config)
                    ///     .$fieldName(handler)
                    ///     /* Set other handlers */
                    ///     .build()
                    ///     .unwrap();
                    /// ## let app: $serviceTypeAssertion = app;
                    /// ```
                    ///
                    pub fn $fieldName<HandlerType, HandlerExtractors, UpgradeExtractors>($selfReceiver, handler: HandlerType) -> Self
                    where
                        HandlerType: #{SmithyHttpServer}::operation::Handler<crate::operation_shape::$structName, HandlerExtractors>,

                        ModelPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            #{SmithyHttpServer}::operation::IntoService<crate::operation_shape::$structName, HandlerType>
                        >,
                        #{ProtocolUpgradeBounds:W}

                    {
                        use #{SmithyHttpServer}::operation::OperationShapeExt;
                        use #{SmithyHttpServer}::plugin::Plugin;
                        let svc = crate::operation_shape::$structName::from_handler(handler);
                        let svc = self.model_plugin.apply(svc);
                        #{ProtocolRouteConstruction:W}
                    }

                    /// Sets the [`$structName`](crate::operation_shape::$structName) operation.
                    ///
                    /// This should be an async function satisfying the [`Handler`](#{SmithyHttpServer}::operation::Handler) trait.
                    /// See the [operation module documentation](#{SmithyHttpServer}::operation) for more information.
                    ///
                    /// ## Example
                    ///
                    /// ```no_run
                    /// use $crateName::{$serviceName, ${serviceName}Config$routerImport};
                    ///
                    #{HandlerImports:W}
                    ///
                    #{HandlerFixed:W}
                    ///
                    /// let config = ${serviceName}Config::builder().build()$unwrapConfigBuilder;
                    /// let svc = #{Tower}::util::service_fn(handler);
                    /// let app = $serviceName::builder(config)
                    ///     .${fieldName}_service(svc)
                    ///     /* Set other handlers */
                    ///     .build()
                    ///     .unwrap();
                    /// ## let app: $serviceTypeAssertion = app;
                    /// ```
                    ///
                    pub fn ${fieldName}_service<S, ServiceExtractors, UpgradeExtractors>($selfReceiver, service: S) -> Self
                    where
                        S: #{SmithyHttpServer}::operation::OperationService<crate::operation_shape::$structName, ServiceExtractors>,

                        ModelPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            #{SmithyHttpServer}::operation::Normalize<crate::operation_shape::$structName, S>
                        >,
                        #{ProtocolUpgradeBounds:W}

                    {
                        use #{SmithyHttpServer}::operation::OperationShapeExt;
                        use #{SmithyHttpServer}::plugin::Plugin;
                        let svc = crate::operation_shape::$structName::from_service(service);
                        let svc = self.model_plugin.apply(svc);
                        #{ProtocolRouteConstruction:W}
                    }

                    #{CustomSetter:W}
                    """,
                    "Router" to protocol.routerType(),
                    "Protocol" to protocol.markerStruct(),
                    "Handler" to handler,
                    "HandlerFixed" to handlerFixed,
                    "HandlerImports" to handlerImports(crateName, operations),
                    "ProtocolUpgradeBounds" to protocolUpgradeBounds(structName),
                    "ProtocolRouteConstruction" to protocolRouteConstruction(fieldName),
                    "CustomSetter" to customSetter(fieldName, structName),
                    *codegenScope,
                )

                // Adds newline between setters.
                rust("")
            }
        }

    private fun buildMethod(): Writable =
        writable {
            if (isMultiProtocol) {
                renderMultiProtocolBuildMethod()
            } else {
                renderSingleProtocolBuildMethod()
            }
        }

    /** Renders `build` for a single-protocol service. */
    private fun RustWriter.renderSingleProtocolBuildMethod() {
        val missingOperationsVariableName = "missing_operation_names"
        val expectMessageVariableName = "unexpected_error_msg"

        val nullabilityChecks =
            writable {
                for (operationShape in operations) {
                    val fieldName = builderFieldNames[operationShape]!!
                    val operationZstTypeName = operationStructNames[operationShape]!!
                    rust(
                        """
                        if self.$fieldName.is_none() {
                            $missingOperationsVariableName.insert(crate::operation_shape::$operationZstTypeName::ID, ".$fieldName()");
                        }
                        """,
                    )
                }
            }
        val routesArrayElements =
            writable {
                for (operationShape in operations) {
                    val fieldName = builderFieldNames[operationShape]!!
                    val (specBuilderFunctionName, _) = requestSpecMap.getValue(operationShape)
                    rust(
                        """
                        ($requestSpecsModuleName::$specBuilderFunctionName(), self.$fieldName.expect($expectMessageVariableName)),
                        """,
                    )
                }
            }

        rustTemplate(
            """
            /// Constructs a [`$serviceName`] from the arguments provided to the builder.
            ///
            /// Forgetting to register a handler for one or more operations will result in an error.
            ///
            /// Check out [`$builderName::build_unchecked`] if you'd prefer the service to return status code 500 when an
            /// unspecified route is requested.
            pub fn build(self) -> #{Result}<
                $serviceName<
                    #{SmithyHttpServer}::routing::RoutingService<
                        #{Router}<L::Service>,
                        #{Protocol},
                    >,
                >,
                MissingOperationsError,
            >
            where
                L: #{Tower}::Layer<#{SmithyHttpServer}::routing::Route<Body>>,
            {
                let router = {
                    use #{SmithyHttpServer}::operation::OperationShape;
                    let mut $missingOperationsVariableName = std::collections::HashMap::new();
                    #{NullabilityChecks:W}
                    if !$missingOperationsVariableName.is_empty() {
                        return Err(MissingOperationsError {
                            operation_names2setter_methods: $missingOperationsVariableName,
                        });
                    }
                    let $expectMessageVariableName = "this should never panic since we are supposed to check beforehand that a handler has been registered for this operation; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues";

                    #{PatternInitializations:W}

                    #{Router}::from_iter([#{RoutesArrayElements:W}])
                };
                let svc = #{SmithyHttpServer}::routing::RoutingService::new(router);
                let svc = svc.map(|s| s.layer(self.layer));
                Ok($serviceName { svc })
            }
            """,
            *codegenScope,
            "Protocol" to protocol.markerStruct(),
            "Router" to protocol.routerType(),
            "NullabilityChecks" to nullabilityChecks,
            "RoutesArrayElements" to routesArrayElements,
            "PatternInitializations" to patternInitializations(),
            *RuntimeType.preludeScope,
        )
    }

    /** Renders `build` for a multi-protocol service. */
    private fun RustWriter.renderMultiProtocolBuildMethod() {
        val missingOperationsVariableName = "missing_operation_names"
        val expectMessageVariableName = "unexpected_error_msg"
        val nullabilityChecks =
            writable {
                for (operationShape in operations) {
                    val fieldName = builderFieldNames[operationShape]!!
                    val operationZstTypeName = operationStructNames[operationShape]!!
                    rust(
                        """
                        if self.$fieldName.is_none() {
                            $missingOperationsVariableName.insert(crate::operation_shape::$operationZstTypeName::ID, ".$fieldName()");
                        }
                        """,
                    )
                }
            }

        val routeInitializations =
            writable {
                for (operationShape in operations) {
                    val fieldName = builderFieldNames.getValue(operationShape)
                    rust("let $protocolRoutesName {")
                    for (protoInfo in protocolInfos) {
                        rust("${protoInfo.identifier}: ${fieldName}_${protoInfo.identifier},")
                    }
                    rust("} = self.$fieldName.expect($expectMessageVariableName);")
                }
            }

        // Generate router construction for each protocol
        val routerConstructions =
            writable {
                for (protoInfo in protocolInfos) {
                    val specsMap = multiProtocolRequestSpecMap[protoInfo.identifier]!!
                    val routerVarName = "${protoInfo.identifier}_router"
                    val routesArrayElements =
                        writable {
                            for (operationShape in operations) {
                                val fieldName = builderFieldNames[operationShape]!!
                                val (specBuilderFunctionName, _) = specsMap.getValue(operationShape)
                                rust(
                                    """
                                    ($requestSpecsModuleName::$specBuilderFunctionName(), ${fieldName}_${protoInfo.identifier}),
                                    """,
                                )
                            }
                        }
                    rustTemplate(
                        """
                        let $routerVarName = #{Router}::from_iter([#{RoutesArrayElements:W}]);
                        """,
                        "Router" to protoInfo.routerType,
                        "RoutesArrayElements" to routesArrayElements,
                    )
                }
            }

        val routingServiceConstructions = routingServiceConstructions(protocolInfos)

        // Install Tower protocol layers in canonical detection order.
        // ServiceBuilder preserves declaration order, so the first protocol is
        // the outermost service and the default not-found service is innermost.
        val multiProtocolConstruction =
            writable {
                rustTemplate("let router = #{Tower}::ServiceBuilder::new()", *codegenScope)
                for (protoInfo in protocolInfos) {
                    val routerVarName = "${protoInfo.identifier}_router"
                    rustTemplate(
                        ".layer(#{SmithyHttpServer}::routing::ProtocolLayer::new(#{Marker}, $routerVarName))",
                        *codegenScope,
                        "Marker" to protoInfo.markerStruct,
                    )
                }
                rustTemplate(
                    ".service(#{SmithyHttpServer}::routing::DefaultNotFoundService);",
                    *codegenScope,
                )
            }

        rustTemplate(
            """
            /// Constructs a [`$serviceName`] from the arguments provided to the builder.
            ///
            /// Requests are dispatched among the generated protocols in detection order.
            ///
            /// Forgetting to register a handler for one or more operations will result in an error.
            ///
            /// Use [`build_unchecked`](Self::build_unchecked) to allow unregistered modeled
            /// operations; requests to those operations return status code 500.
            pub fn build(self) -> #{Result}<
                $serviceName<$routerName<L::Service>>,
                MissingOperationsError,
            >
            where
                L: #{Tower}::Layer<#{SmithyHttpServer}::routing::Route<Body>>,
                L::Service: Clone,
            {
                use #{SmithyHttpServer}::operation::OperationShape;
                let mut $missingOperationsVariableName = std::collections::HashMap::new();
                #{NullabilityChecks:W}
                if !$missingOperationsVariableName.is_empty() {
                    return Err(MissingOperationsError {
                        operation_names2setter_methods: $missingOperationsVariableName,
                    });
                }
                let $expectMessageVariableName = "this should never panic since we are supposed to check beforehand that a handler has been registered for this operation; please file a bug report under https://github.com/smithy-lang/smithy-rs/issues";

                #{RouteInitializations:W}
                #{PatternInitializations:W}

                #{RouterConstructions:W}

                // Apply the configured layer to each protocol router.
                #{RoutingServiceConstructions:W}

                // Nest protocol services in detection order.
                #{MultiProtocolConstruction:W}

                Ok($serviceName { svc: router })
            }
            """,
            *codegenScope,
            "NullabilityChecks" to nullabilityChecks,
            "RouteInitializations" to routeInitializations,
            "PatternInitializations" to patternInitializations(),
            "RouterConstructions" to routerConstructions,
            "RoutingServiceConstructions" to routingServiceConstructions,
            "MultiProtocolConstruction" to multiProtocolConstruction,
            *RuntimeType.preludeScope,
        )
    }

    /**
     * Renders `PatternString::compile_regex()` function calls for every
     * `@pattern`-constrained string shape in the service closure.
     */
    @Suppress("DEPRECATION")
    private fun patternInitializations(): Writable {
        val patterns =
            Walker(model).walkShapes(service)
                .filter { shape -> shape is StringShape && shape.hasTrait<PatternTrait>() && !shape.hasTrait<software.amazon.smithy.model.traits.EnumTrait>() }
                .map { shape -> codegenContext.constrainedShapeSymbolProvider.toSymbol(shape) }
                .map { symbol ->
                    writable {
                        rustTemplate("#{Type}::compile_regex();", "Type" to symbol)
                    }
                }

        patterns.letIf(patterns.isNotEmpty()) {
            val docs = listOf(writable { rust("// Eagerly initialize regexes for `@pattern` strings.") })

            docs + patterns
        }

        return patterns.join("")
    }

    private fun buildUncheckedMethod(): Writable =
        writable {
            if (isMultiProtocol) {
                renderMultiProtocolBuildUncheckedMethod()
            } else {
                renderSingleProtocolBuildUncheckedMethod()
            }
        }

    /** Render build_unchecked for single-protocol services. */
    private fun RustWriter.renderSingleProtocolBuildUncheckedMethod() {
        val pairs =
            writable {
                for (operationShape in operations) {
                    val fieldName = builderFieldNames[operationShape]!!
                    val (specBuilderFunctionName, _) = requestSpecMap.getValue(operationShape)
                    rustTemplate(
                        """
                        (
                                $requestSpecsModuleName::$specBuilderFunctionName(),
                                self.$fieldName.unwrap_or_else(|| {
                                    let svc = #{SmithyHttpServer}::operation::MissingFailure::<#{Protocol}>::default();
                                    #{SmithyHttpServer}::routing::Route::new(svc)
                                })
                            ),
                        """,
                        "SmithyHttpServer" to smithyHttpServer,
                        "Protocol" to protocol.markerStruct(),
                    )
                }
            }
        rustTemplate(
            """
            /// Constructs a [`$serviceName`] from the arguments provided to the builder.
            /// Operations without a handler default to returning 500 Internal Server Error to the caller.
            ///
            /// Check out [`$builderName::build`] if you'd prefer the builder to fail if one or more operations do
            /// not have a registered handler.
            pub fn build_unchecked(self) -> $serviceName<L::Service>
            where
                Body: Send + 'static,
                L: #{Tower}::Layer<
                    #{SmithyHttpServer}::routing::RoutingService<#{Router}<#{SmithyHttpServer}::routing::Route<Body>>, #{Protocol}>
                >
            {
                let router = #{Router}::from_iter([#{Pairs:W}]);
                let svc = self
                    .layer
                    .layer(#{SmithyHttpServer}::routing::RoutingService::new(router));
                $serviceName { svc }
            }
            """,
            *codegenScope,
            "Protocol" to protocol.markerStruct(),
            "Router" to protocol.routerType(),
            "Pairs" to pairs,
        )
    }

    /** Renders `build_unchecked` for a multi-protocol service. */
    private fun RustWriter.renderMultiProtocolBuildUncheckedMethod() {
        val routeInitializations =
            writable {
                for (operationShape in operations) {
                    val fieldName = builderFieldNames.getValue(operationShape)
                    rust("let $protocolRoutesName {")
                    for (protoInfo in protocolInfos) {
                        rust("${protoInfo.identifier}: ${fieldName}_${protoInfo.identifier},")
                    }
                    rust("} = self.$fieldName.unwrap_or_else(|| $protocolRoutesName {")
                    for (protoInfo in protocolInfos) {
                        rustTemplate(
                            """
                            ${protoInfo.identifier}: #{SmithyHttpServer}::routing::Route::new(
                                #{SmithyHttpServer}::operation::MissingFailure::<#{Protocol}>::default()
                            ),
                            """,
                            *codegenScope,
                            "Protocol" to protoInfo.markerStruct,
                        )
                    }
                    rust("});")
                }
            }

        // Generate router construction for each protocol
        val routerConstructions =
            writable {
                for (protoInfo in protocolInfos) {
                    val specsMap = multiProtocolRequestSpecMap[protoInfo.identifier]!!
                    val routerVarName = "${protoInfo.identifier}_router"
                    val pairs =
                        writable {
                            for (operationShape in operations) {
                                val fieldName = builderFieldNames[operationShape]!!
                                val (specBuilderFunctionName, _) = specsMap.getValue(operationShape)
                                rust(
                                    """
                                    (
                                        $requestSpecsModuleName::$specBuilderFunctionName(),
                                        ${fieldName}_${protoInfo.identifier}
                                    ),
                                    """,
                                )
                            }
                        }
                    rustTemplate(
                        """
                        let $routerVarName = #{Router}::from_iter([#{Pairs:W}]);
                        """,
                        "Router" to protoInfo.routerType,
                        "Pairs" to pairs,
                    )
                }
            }

        val routingServiceConstructions = routingServiceConstructions(protocolInfos)

        // Install Tower protocol layers in canonical detection order.
        // ServiceBuilder preserves declaration order, so the first protocol is
        // the outermost service and the default not-found service is innermost.
        val multiProtocolConstruction =
            writable {
                rustTemplate("let router = #{Tower}::ServiceBuilder::new()", *codegenScope)
                for (protoInfo in protocolInfos) {
                    val routerVarName = "${protoInfo.identifier}_router"
                    rustTemplate(
                        ".layer(#{SmithyHttpServer}::routing::ProtocolLayer::new(#{Marker}, $routerVarName))",
                        *codegenScope,
                        "Marker" to protoInfo.markerStruct,
                    )
                }
                rustTemplate(
                    ".service(#{SmithyHttpServer}::routing::DefaultNotFoundService);",
                    *codegenScope,
                )
            }

        rustTemplate(
            """
            /// Constructs a [`$serviceName`] from the arguments provided to the builder.
            /// Operations without a handler default to returning 500 Internal Server Error to the caller.
            ///
            /// Requests are dispatched among the generated protocols in detection order.
            ///
            /// Check out [`$builderName::build`] if you'd prefer the builder to fail if one or more operations do
            /// not have a registered handler.
            pub fn build_unchecked(self) -> $serviceName<$routerName<L::Service>>
            where
                Body: #{HttpBody}::Body<Data = #{Bytes}> + Send + 'static,
                Body::Error: Into<#{SmithyHttpServer}::BoxError>,
                L: #{Tower}::Layer<#{SmithyHttpServer}::routing::Route<Body>>,
                L::Service: Clone,
            {
                #{RouteInitializations:W}

                #{RouterConstructions:W}

                // Apply the configured layer to each protocol router.
                #{RoutingServiceConstructions:W}

                // Nest protocol services in detection order.
                #{MultiProtocolConstruction:W}

                $serviceName { svc: router }
            }
            """,
            *codegenScope,
            "RouteInitializations" to routeInitializations,
            "RouterConstructions" to routerConstructions,
            "RoutingServiceConstructions" to routingServiceConstructions,
            "MultiProtocolConstruction" to multiProtocolConstruction,
        )
    }

    /** Returns a `Writable` containing the builder struct definition and its implementations. */
    private fun builder(): Writable =
        writable {
            val builderGenerics = listOf("Body", "L", "HttpPl", "ModelPl").joinToString(", ")
            rustTemplate(
                """
                /// The service builder for [`$serviceName`].
                ///
                /// Constructed via [`$serviceName::builder`].
                pub struct $builderName<$builderGenerics> {
                    ${builderFields.joinToString(", ")},
                    layer: L,
                    http_plugin: HttpPl,
                    model_plugin: ModelPl
                }

                impl<$builderGenerics> $builderName<$builderGenerics> {
                    #{Setters:W}
                }

                impl<$builderGenerics> $builderName<$builderGenerics> {
                    #{BuildMethod:W}

                    #{BuildUncheckedMethod:W}
                }
                """,
                "Setters" to builderSetters(),
                "BuildMethod" to buildMethod(),
                "BuildUncheckedMethod" to buildUncheckedMethod(),
                *codegenScope,
            )
        }

    private fun requestSpecsModule(): Writable =
        writable {
            if (isMultiProtocol) {
                // For multi-protocol, generate specs for each protocol
                val functions =
                    writable {
                        for ((_, specsMap) in multiProtocolRequestSpecMap) {
                            for ((_, function) in specsMap.values) {
                                rustTemplate(
                                    """
                                    pub(super) #{Function:W}
                                    """,
                                    "Function" to function,
                                )
                            }
                        }
                    }
                rustTemplate(
                    """
                    mod $requestSpecsModuleName {
                        #{SpecFunctions:W}
                    }
                    """,
                    "SpecFunctions" to functions,
                )
            } else {
                // Single protocol - existing behavior
                val functions =
                    writable {
                        for ((_, function) in requestSpecMap.values) {
                            rustTemplate(
                                """
                                pub(super) #{Function:W}
                                """,
                                "Function" to function,
                            )
                        }
                    }
                rustTemplate(
                    """
                    mod $requestSpecsModuleName {
                        #{SpecFunctions:W}
                    }
                    """,
                    "SpecFunctions" to functions,
                )
            }
        }

    /** Returns a `Writable` comma delimited sequence of `builder_field: None`. */
    private fun notSetFields(): Writable =
        builderFieldNames.values.map {
            writable {
                rustTemplate(
                    "$it: None",
                    *codegenScope,
                )
            }
        }.join(", ")

    /** Returns a `Writable` containing the service struct definition and its implementations. */
    private fun serviceStruct(): Writable =
        writable {
            documentShape(service, model)

            if (isMultiProtocol) {
                // Multi-protocol: use the type alias as default
                rustTemplate(
                    """
                    ///
                    /// See the [root](crate) documentation for more information.
                    ///
                    /// This service supports multiple protocols.
                    ##[derive(Clone)]
                    pub struct $serviceName<S = $routerName> {
                        // This is the router wrapped by layers.
                        svc: S,
                    }
                    """,
                    *codegenScope,
                )
            } else {
                // Single protocol: preserve the established concrete default service type.
                rustTemplate(
                    """
                    ///
                    /// See the [root](crate) documentation for more information.
                    ##[derive(Clone)]
                    pub struct $serviceName<
                        S = #{SmithyHttpServer}::routing::RoutingService<
                            #{Router}<
                                #{SmithyHttpServer}::routing::Route<
                                    #{SmithyHttpServer}::body::BoxBody
                                >,
                            >,
                            #{Protocol},
                        >
                    > {
                        // This is the router wrapped by layers.
                        svc: S,
                    }
                    """,
                    "Router" to protocol.routerType(),
                    "Protocol" to protocol.markerStruct(),
                    *codegenScope,
                )
            }

            rust("")
            rustTemplate(
                """
                impl $serviceName<()> {
                    /// Constructs a builder for [`$serviceName`].
                    /// You must specify a configuration object holding any plugins and layers that should be applied
                    /// to the operations in this service.
                    pub fn builder<
                        Body,
                        L,
                        HttpPl: #{SmithyHttpServer}::plugin::HttpMarker,
                        ModelPl: #{SmithyHttpServer}::plugin::ModelMarker,
                    >(
                        config: ${serviceName}Config<L, HttpPl, ModelPl>,
                    ) -> $builderName<Body, L, HttpPl, ModelPl> {
                        $builderName {
                            #{NotSetFields1:W},
                            layer: config.layers,
                            http_plugin: config.http_plugins,
                            model_plugin: config.model_plugins,
                        }
                    }

                    /// Constructs a builder for [`$serviceName`].
                    /// You must specify what plugins should be applied to the operations in this service.
                    ///
                    /// Use [`$serviceName::builder_without_plugins`] if you don't need to apply plugins.
                    ///
                    /// Check out [`HttpPlugins`](#{SmithyHttpServer}::plugin::HttpPlugins) and
                    /// [`ModelPlugins`](#{SmithyHttpServer}::plugin::ModelPlugins) if you need to apply
                    /// multiple plugins.
                    ##[deprecated(
                        since = "0.57.0",
                        note = "please use the `builder` constructor and register plugins on the `${serviceName}Config` object instead; see https://github.com/smithy-lang/smithy-rs/discussions/3096"
                    )]
                    pub fn builder_with_plugins<
                        Body,
                        HttpPl: #{SmithyHttpServer}::plugin::HttpMarker,
                        ModelPl: #{SmithyHttpServer}::plugin::ModelMarker
                    >(
                        http_plugin: HttpPl,
                        model_plugin: ModelPl
                    ) -> $builderName<Body, #{Tower}::layer::util::Identity, HttpPl, ModelPl> {
                        $builderName {
                            #{NotSetFields2:W},
                            layer: #{Tower}::layer::util::Identity::new(),
                            http_plugin,
                            model_plugin
                        }
                    }

                    /// Constructs a builder for [`$serviceName`].
                    ///
                    /// Use [`$serviceName::builder_with_plugins`] if you need to specify plugins.
                    ##[deprecated(
                        since = "0.57.0",
                        note = "please use the `builder` constructor instead; see https://github.com/smithy-lang/smithy-rs/discussions/3096"
                    )]
                    pub fn builder_without_plugins<Body>() -> $builderName<
                        Body,
                        #{Tower}::layer::util::Identity,
                        #{SmithyHttpServer}::plugin::IdentityPlugin,
                        #{SmithyHttpServer}::plugin::IdentityPlugin
                    > {
                        Self::builder_with_plugins(#{SmithyHttpServer}::plugin::IdentityPlugin, #{SmithyHttpServer}::plugin::IdentityPlugin)
                    }
                }

                impl<S> $serviceName<S> {
                    /// Converts [`$serviceName`] into a [`MakeService`](tower::make::MakeService).
                    pub fn into_make_service(self) -> #{SmithyHttpServer}::routing::IntoMakeService<Self> {
                        #{SmithyHttpServer}::routing::IntoMakeService::new(self)
                    }


                    /// Converts [`$serviceName`] into a [`MakeService`](tower::make::MakeService) with [`ConnectInfo`](#{SmithyHttpServer}::request::connect_info::ConnectInfo).
                    pub fn into_make_service_with_connect_info<C>(self) -> #{SmithyHttpServer}::routing::IntoMakeServiceWithConnectInfo<Self, C> {
                        #{SmithyHttpServer}::routing::IntoMakeServiceWithConnectInfo::new(self)
                    }
                }
                """,
                "NotSetFields1" to notSetFields(),
                "NotSetFields2" to notSetFields(),
                *codegenScope,
            )

            // Protocol-specific layer and boxed methods only for single-protocol services
            if (!isMultiProtocol) {
                rust("")
                rustTemplate(
                    """
                    impl<S>
                        $serviceName<
                            #{SmithyHttpServer}::routing::RoutingService<
                                #{Router}<S>,
                                #{Protocol},
                            >,
                        >
                    {
                        /// Applies a [`Layer`](#{Tower}::Layer) uniformly to all routes.
                        ##[deprecated(
                            since = "0.57.0",
                            note = "please add layers to the `${serviceName}Config` object instead; see https://github.com/smithy-lang/smithy-rs/discussions/3096"
                        )]
                        pub fn layer<L>(
                            self,
                            layer: &L,
                        ) -> $serviceName<
                            #{SmithyHttpServer}::routing::RoutingService<
                                #{Router}<L::Service>,
                                #{Protocol},
                            >,
                        >
                        where
                            L: #{Tower}::Layer<S>,
                        {
                            $serviceName {
                                svc: self.svc.map(|s| s.layer(layer)),
                            }
                        }

                        /// Applies [`Route::new`](#{SmithyHttpServer}::routing::Route::new) to all routes.
                        ///
                        /// This has the effect of erasing all types accumulated via layers.
                        pub fn boxed<B>(
                            self,
                        ) -> $serviceName<
                            #{SmithyHttpServer}::routing::RoutingService<
                                #{Router}<
                                    #{SmithyHttpServer}::routing::Route<B>,
                                >,
                                #{Protocol},
                            >,
                        >
                        where
                            S: #{Tower}::Service<
                                #{Http}::Request<B>,
                                Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>,
                                Error = std::convert::Infallible,
                            >,
                            S: Clone + Send + 'static,
                            S::Future: Send + 'static,
                        {
                            self.layer(&::tower::layer::layer_fn(
                                #{SmithyHttpServer}::routing::Route::new,
                            ))
                        }
                    }
                    """,
                    "Router" to protocol.routerType(),
                    "Protocol" to protocol.markerStruct(),
                    *codegenScope,
                )
            }

            rust("")
            rustTemplate(
                """
                impl<S, R> #{Tower}::Service<R> for $serviceName<S>
                where
                    S: #{Tower}::Service<R>,
                {
                    type Response = S::Response;
                    type Error = S::Error;
                    type Future = S::Future;

                    fn poll_ready(&mut self, cx: &mut std::task::Context) -> std::task::Poll<#{Result}<(), Self::Error>> {
                        self.svc.poll_ready(cx)
                    }

                    fn call(&mut self, request: R) -> Self::Future {
                        self.svc.call(request)
                    }
                }
                """,
                *codegenScope,
            )
        }

    private fun missingOperationsError(): Writable =
        writable {
            rustTemplate(
                """
                /// The error encountered when calling the [`$builderName::build`] method if one or more operation handlers are not
                /// specified.
                ##[derive(Debug)]
                pub struct MissingOperationsError {
                    operation_names2setter_methods: std::collections::HashMap<#{SmithyHttpServer}::shape_id::ShapeId, &'static str>,
                }

                impl std::fmt::Display for MissingOperationsError {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(
                            f,
                            "You must specify a handler for all operations attached to `$serviceName`.\n\
                            We are missing handlers for the following operations:\n",
                        )?;
                        for operation_name in self.operation_names2setter_methods.keys() {
                            writeln!(f, "- {}", operation_name.absolute())?;
                        }

                        writeln!(f, "\nUse the dedicated methods on `$builderName` to register the missing handlers:")?;
                        for setter_name in self.operation_names2setter_methods.values() {
                            writeln!(f, "- {}", setter_name)?;
                        }
                        Ok(())
                    }
                }

                impl std::error::Error for MissingOperationsError {}
                """,
                *codegenScope,
            )
        }

    private fun serviceShapeImpl(): Writable =
        writable {
            val namespace = serviceId.namespace
            val name = serviceId.name
            val absolute = serviceId.toString().replace("#", "##")
            val version = codegenContext.serviceShape.version?.let { "Some(\"$it\")" } ?: "None"
            rustTemplate(
                """
                impl<S> #{SmithyHttpServer}::service::ServiceShape for $serviceName<S> {
                    const ID: #{SmithyHttpServer}::shape_id::ShapeId = #{SmithyHttpServer}::shape_id::ShapeId::new("$absolute", "$namespace", "$name");

                    const VERSION: Option<&'static str> = $version;

                    type Protocol = #{Protocol};

                    type Operations = Operation;
                }
                """,
                "Protocol" to protocol.markerStruct(),
                *codegenScope,
            )
        }

    private fun operationEnum(): Writable =
        writable {
            val operations = operationStructNames.values.joinToString(",")
            val matchArms: Writable =
                operationStructNames.map {
                        (shape, name) ->
                    writable {
                        val absolute = shape.id.toString().replace("#", "##")
                        rustTemplate(
                            """
                            Operation::$name => #{SmithyHttpServer}::shape_id::ShapeId::new("$absolute", "${shape.id.namespace}", "${shape.id.name}")
                            """,
                            *codegenScope,
                        )
                    }
                }.join(",")
            rustTemplate(
                """
                /// An enumeration of all [operations](https://smithy.io/2.0/spec/service-types.html##operation) in $serviceName.
                ##[allow(clippy::enum_variant_names)]
                ##[derive(Debug, PartialEq, Eq, Clone, Copy)]
                pub enum Operation {
                    $operations
                }

                impl Operation {
                    /// Returns the [operations](https://smithy.io/2.0/spec/service-types.html##operation) [`ShapeId`](#{SmithyHttpServer}::shape_id::ShapeId).
                    pub fn shape_id(&self) -> #{SmithyHttpServer}::shape_id::ShapeId {
                        match self {
                            #{Arms}
                        }
                    }
                }
                """,
                *codegenScope,
                "Arms" to matchArms,
            )

            for ((_, value) in operationStructNames) {
                rustTemplate(
                    """
                    impl<L> #{SmithyHttpServer}::service::ContainsOperation<crate::operation_shape::$value>
                        for $serviceName<L>
                    {
                        const VALUE: Operation = Operation::$value;
                    }
                    """,
                    *codegenScope,
                )
            }
        }

    fun render(writer: RustWriter) {
        if (isMultiProtocol) {
            writer.rustTemplate(
                """
                #{RouterTypeAlias:W}

                #{ProtocolRoutes:W}

                #{Builder:W}

                #{MissingOperationsError:W}

                #{RequestSpecs:W}

                #{Struct:W}

                #{Operations}

                #{ServiceImpl}
                """,
                "RouterTypeAlias" to routerTypeAlias(),
                "ProtocolRoutes" to protocolRoutes(),
                "Builder" to builder(),
                "MissingOperationsError" to missingOperationsError(),
                "RequestSpecs" to requestSpecsModule(),
                "Struct" to serviceStruct(),
                "Operations" to operationEnum(),
                "ServiceImpl" to serviceShapeImpl(),
                *codegenScope,
            )
        } else {
            writer.rustTemplate(
                """
                #{Builder:W}

                #{MissingOperationsError:W}

                #{RequestSpecs:W}

                #{Struct:W}

                #{Operations}

                #{ServiceImpl}
                """,
                "Builder" to builder(),
                "MissingOperationsError" to missingOperationsError(),
                "RequestSpecs" to requestSpecsModule(),
                "Struct" to serviceStruct(),
                "Operations" to operationEnum(),
                "ServiceImpl" to serviceShapeImpl(),
                *codegenScope,
            )
        }
    }
}

/**
 * Returns a writable to import the necessary modules used by a handler implementation stub.
 *
 * ```rust
 * use my_service::{input, output, error};
 * ```
 */
fun handlerImports(
    crateName: String,
    operations: Collection<OperationShape>,
    commentToken: String = "///",
) = writable {
    val hasErrors = operations.any { it.errors.isNotEmpty() }
    val errorImport = if (hasErrors) ", ${ErrorModule.name}" else ""
    if (operations.isNotEmpty()) {
        rust("$commentToken use $crateName::{${InputModule.name}, ${OutputModule.name}$errorImport};")
    }
}
