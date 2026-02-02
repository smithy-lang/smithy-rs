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

    // Multi-protocol support
    private val isMultiProtocol = codegenContext.isMultiProtocol
    private val allProtocols = codegenContext.allProtocols

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

    /** A `Writable` block of "field: Type" for the builder. */
    private val builderFields =
        builderFieldNames.values.map { name -> "$name: Option<#{SmithyHttpServer}::routing::Route<Body>>" }

    /** The name of the local private module containing the functions that return the request for each operation */
    private val requestSpecsModuleName = "request_specs"

    /** Associate each operation with a function that returns its request spec. */
    private val requestSpecMap: Map<OperationShape, Pair<String, Writable>> =
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
            Pair(functionName, functionBody)
        }

    /**
     * For multi-protocol support: Generate request specs for each protocol.
     * Maps protocol module path -> (operation -> (function name, function body))
     */
    private val multiProtocolRequestSpecMap: Map<String, Map<OperationShape, Pair<String, Writable>>> by lazy {
        if (!isMultiProtocol) {
            emptyMap()
        } else {
            allProtocols.associate { proto ->
                val protoModulePath = proto.protocolModulePath
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
                        // Add protocol suffix to function name to avoid collisions
                        val functionName =
                            RustReservedWords.escapeIfNeeded(operationName.toSnakeCase()) + "_" + protoModulePath
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
                        Pair(functionName, functionBody)
                    }
                protoModulePath to specsMap
            }
        }
    }

    /**
     * Helper to determine the protocol type name for multi-protocol router generation.
     * Returns pairs of (has_protocol, protocol_marker_path, router_type_path, builder_method_name)
     */
    private fun getProtocolInfo(): List<ProtocolRouterInfo> {
        if (!isMultiProtocol) return emptyList()

        return allProtocols.map { proto ->
            val modulePath = proto.protocolModulePath
            val markerStruct = proto.markerStruct()
            val routerType = proto.routerType()
            val builderMethod =
                when (modulePath) {
                    "rest_json_1" -> "with_rest_json1"
                    "rest_xml" -> "with_rest_xml"
                    "aws_json_10" -> "with_aws_json_10"
                    "aws_json_11" -> "with_aws_json_11"
                    "rpc_v2_cbor" -> "with_rpc_v2_cbor"
                    else -> throw IllegalStateException("Unknown protocol module path: $modulePath")
                }
            ProtocolRouterInfo(modulePath, markerStruct, routerType, builderMethod)
        }
    }

    /** Helper data class for protocol router info */
    private data class ProtocolRouterInfo(
        val modulePath: String,
        val markerStruct: RuntimeType,
        val routerType: RuntimeType,
        val builderMethod: String,
    )

    /**
     * Generate the router type alias for this service.
     *
     * For single-protocol services, this generates a type alias wrapping `RoutingService`.
     * For multi-protocol services, this generates a type alias wrapping `MultiProtocolService`.
     *
     * The router is generic over `S`, the service type stored in the underlying router.
     * This defaults to `Route` (which uses `hyper::body::Incoming`) for standard HTTP server use cases.
     */
    private fun routerTypeAlias(): Writable =
        writable {
            if (isMultiProtocol) {
                val protocolInfos = getProtocolInfo()

                // Build the MultiProtocolService generic parameters using S (service type)
                // Order matches detection priority: RpcV2, AwsJson11, AwsJson10, RestJson, RestXml
                // Map module paths to their public routing service type aliases
                val routerTypeParams =
                    listOf("rpc_v2_cbor", "aws_json_11", "aws_json_10", "rest_json_1", "rest_xml").map { modulePath ->
                        val matchingProtocol = protocolInfos.find { it.modulePath == modulePath }
                        if (matchingProtocol != null) {
                            // Use public type aliases from aws_smithy_http_server::routing
                            when (modulePath) {
                                "rpc_v2_cbor" -> "#{SmithyHttpServer}::routing::CborRoutingService<S>"
                                "aws_json_11" -> "#{SmithyHttpServer}::routing::AwsJson11RoutingService<S>"
                                "aws_json_10" -> "#{SmithyHttpServer}::routing::AwsJson10RoutingService<S>"
                                "rest_json_1" -> "#{SmithyHttpServer}::routing::RestJson1RoutingService<S>"
                                "rest_xml" -> "#{SmithyHttpServer}::routing::RestXmlRoutingService<S>"
                                else -> throw IllegalStateException("Unknown protocol module path: $modulePath")
                            }
                        } else {
                            "()" // Protocol not used by this service
                        }
                    }

                rustTemplate(
                    """
                    /// Type alias for the multi-protocol router used by this service.
                    ///
                    /// This type handles routing requests to the appropriate protocol handler
                    /// based on request characteristics (headers, content-type, URI path).
                    ///
                    /// The type parameter `S` is the service type stored in the underlying routers,
                    /// defaulting to `Route` (which uses `hyper::body::Incoming`) for standard HTTP server use cases.
                    pub type $routerName<S = #{SmithyHttpServer}::routing::Route> =
                        #{SmithyHttpServer}::routing::MultiProtocolService<
                            ${routerTypeParams.joinToString(",\n                            ")},
                            #{SmithyHttpServer}::routing::DefaultNotFoundService,
                        >;
                    """,
                    *codegenScope,
                )
            } else {
                // Single-protocol: generate type alias for RoutingService
                rustTemplate(
                    """
                    /// Type alias for the router used by this service.
                    ///
                    /// This type handles routing requests to the appropriate operation handler
                    /// based on the request URI and method.
                    ///
                    /// The type parameter `S` is the service type stored in the underlying router,
                    /// defaulting to `Route` (which uses `hyper::body::Incoming`) for standard HTTP server use cases.
                    pub type $routerName<S = #{SmithyHttpServer}::routing::Route> =
                        #{SmithyHttpServer}::routing::RoutingService<
                            #{Router}<S>,
                            #{Protocol},
                        >;
                    """,
                    "Router" to protocol.routerType(),
                    "Protocol" to protocol.markerStruct(),
                    *codegenScope,
                )
            }
        }

    /** A `Writable` block containing all the `Handler` and `Operation` setters for the builder. */
    private fun builderSetters(): Writable =
        writable {
            for ((operationShape, structName) in operationStructNames) {
                val fieldName = builderFieldNames[operationShape]
                val docHandler = DocHandlerGenerator(codegenContext, operationShape, "handler", "///")
                val handler = docHandler.docSignature()
                val handlerFixed = docHandler.docFixedSignature()
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
                    /// use $crateName::{$serviceName, ${serviceName}Config};
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
                    /// ## let app: $serviceName<$routerName> = app;
                    /// ```
                    ///
                    pub fn $fieldName<HandlerType, HandlerExtractors, UpgradeExtractors>(self, handler: HandlerType) -> Self
                    where
                        HandlerType: #{SmithyHttpServer}::operation::Handler<crate::operation_shape::$structName, HandlerExtractors>,

                        ModelPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            #{SmithyHttpServer}::operation::IntoService<crate::operation_shape::$structName, HandlerType>
                        >,
                        #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            ModelPl::Output
                        >,
                        HttpPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            <
                                #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>
                                as #{SmithyHttpServer}::plugin::Plugin<
                                    $serviceName<L>,
                                    crate::operation_shape::$structName,
                                    ModelPl::Output
                                >
                            >::Output
                        >,

                        HttpPl::Output: #{Tower}::Service<#{Http}::Request<Body>, Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>, Error = ::std::convert::Infallible> + Clone + Send + 'static,
                        <HttpPl::Output as #{Tower}::Service<#{Http}::Request<Body>>>::Future: Send + 'static,

                    {
                        use #{SmithyHttpServer}::operation::OperationShapeExt;
                        use #{SmithyHttpServer}::plugin::Plugin;
                        let svc = crate::operation_shape::$structName::from_handler(handler);
                        let svc = self.model_plugin.apply(svc);
                        let svc = #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>::new().apply(svc);
                        let svc = self.http_plugin.apply(svc);
                        self.${fieldName}_custom(svc)
                    }

                    /// Sets the [`$structName`](crate::operation_shape::$structName) operation.
                    ///
                    /// This should be an async function satisfying the [`Handler`](#{SmithyHttpServer}::operation::Handler) trait.
                    /// See the [operation module documentation](#{SmithyHttpServer}::operation) for more information.
                    ///
                    /// ## Example
                    ///
                    /// ```no_run
                    /// use $crateName::{$serviceName, ${serviceName}Config};
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
                    /// ## let app: $serviceName<$routerName> = app;
                    /// ```
                    ///
                    pub fn ${fieldName}_service<S, ServiceExtractors, UpgradeExtractors>(self, service: S) -> Self
                    where
                        S: #{SmithyHttpServer}::operation::OperationService<crate::operation_shape::$structName, ServiceExtractors>,

                        ModelPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            #{SmithyHttpServer}::operation::Normalize<crate::operation_shape::$structName, S>
                        >,
                        #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            ModelPl::Output
                        >,
                        HttpPl: #{SmithyHttpServer}::plugin::Plugin<
                            $serviceName<L>,
                            crate::operation_shape::$structName,
                            <
                                #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>
                                as #{SmithyHttpServer}::plugin::Plugin<
                                    $serviceName<L>,
                                    crate::operation_shape::$structName,
                                    ModelPl::Output
                                >
                            >::Output
                        >,

                        HttpPl::Output: #{Tower}::Service<#{Http}::Request<Body>, Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>, Error = ::std::convert::Infallible> + Clone + Send + 'static,
                        <HttpPl::Output as #{Tower}::Service<#{Http}::Request<Body>>>::Future: Send + 'static,

                    {
                        use #{SmithyHttpServer}::operation::OperationShapeExt;
                        use #{SmithyHttpServer}::plugin::Plugin;
                        let svc = crate::operation_shape::$structName::from_service(service);
                        let svc = self.model_plugin.apply(svc);
                        let svc = #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>::new().apply(svc);
                        let svc = self.http_plugin.apply(svc);
                        self.${fieldName}_custom(svc)
                    }

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
                    "Router" to protocol.routerType(),
                    "Protocol" to protocol.markerStruct(),
                    "Handler" to handler,
                    "HandlerFixed" to handlerFixed,
                    "HandlerImports" to handlerImports(crateName, operations),
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

    /** Render the build method for single-protocol services (unchanged from before). */
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
                $serviceName<$routerName<L::Service>>,
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

    /** Render the build method for multi-protocol services. */
    private fun RustWriter.renderMultiProtocolBuildMethod() {
        val missingOperationsVariableName = "missing_operation_names"
        val expectMessageVariableName = "unexpected_error_msg"
        val protocolInfos = getProtocolInfo()

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

        // Generate router construction for each protocol
        val routerConstructions =
            writable {
                for (protoInfo in protocolInfos) {
                    val specsMap = multiProtocolRequestSpecMap[protoInfo.modulePath]!!
                    val routerVarName = "${protoInfo.modulePath}_router"
                    val routesArrayElements =
                        writable {
                            for (operationShape in operations) {
                                val fieldName = builderFieldNames[operationShape]!!
                                val (specBuilderFunctionName, _) = specsMap.getValue(operationShape)
                                rust(
                                    """
                                    ($requestSpecsModuleName::$specBuilderFunctionName(), self.$fieldName.clone().expect($expectMessageVariableName)),
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

        // Generate the RoutingService construction with layer application for each protocol
        val routingServiceConstructions =
            writable {
                for (protoInfo in protocolInfos) {
                    val routerVarName = "${protoInfo.modulePath}_router"
                    val svcVarName = "${protoInfo.modulePath}_svc"
                    rustTemplate(
                        """
                        let $svcVarName = #{SmithyHttpServer}::routing::RoutingService::new($routerVarName);
                        let $svcVarName = $svcVarName.map(|s| s.layer(&self.layer));
                        """,
                        *codegenScope,
                    )
                }
            }

        // Generate the MultiProtocolService construction
        val multiProtocolConstruction =
            writable {
                rustTemplate(
                    "let router = #{SmithyHttpServer}::routing::MultiProtocolService::new()",
                    *codegenScope
                )
                for (protoInfo in protocolInfos) {
                    val svcVarName = "${protoInfo.modulePath}_svc"
                    rustTemplate(
                        """
                            .${protoInfo.builderMethod}($svcVarName)
                        """,
                        *codegenScope,
                    )
                }
                rust(";")
            }

        rustTemplate(
            """
            /// Constructs a [`$serviceName`] from the arguments provided to the builder.
            ///
            /// This service supports multiple protocols: ${protocolInfos.joinToString(", ") { it.modulePath }}.
            ///
            /// Forgetting to register a handler for one or more operations will result in an error.
            ///
            /// Check out [`$builderName::build_unchecked`] if you'd prefer the service to return status code 500 when an
            /// unspecified route is requested.
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

                #{PatternInitializations:W}

                // Build router for each protocol
                #{RouterConstructions:W}

                // Wrap each router in RoutingService and apply user's layer
                #{RoutingServiceConstructions:W}

                // Combine into MultiProtocolService
                #{MultiProtocolConstruction:W}

                Ok($serviceName { svc: router })
            }
            """,
            *codegenScope,
            "NullabilityChecks" to nullabilityChecks,
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
                L: #{Tower}::Layer<$routerName<#{SmithyHttpServer}::routing::Route<Body>>>
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

    /** Render build_unchecked for multi-protocol services. */
    private fun RustWriter.renderMultiProtocolBuildUncheckedMethod() {
        val protocolInfos = getProtocolInfo()
        // Use the first protocol as the default for MissingFailure
        val defaultProtocol = protocolInfos.first()

        // Generate router construction for each protocol
        val routerConstructions =
            writable {
                for (protoInfo in protocolInfos) {
                    val specsMap = multiProtocolRequestSpecMap[protoInfo.modulePath]!!
                    val routerVarName = "${protoInfo.modulePath}_router"
                    val pairs =
                        writable {
                            for (operationShape in operations) {
                                val fieldName = builderFieldNames[operationShape]!!
                                val (specBuilderFunctionName, _) = specsMap.getValue(operationShape)
                                rustTemplate(
                                    """
                                    (
                                        $requestSpecsModuleName::$specBuilderFunctionName(),
                                        self.$fieldName.clone().unwrap_or_else(|| {
                                            let svc = #{SmithyHttpServer}::operation::MissingFailure::<#{Protocol}>::default();
                                            #{SmithyHttpServer}::routing::Route::new(svc)
                                        })
                                    ),
                                    """,
                                    "SmithyHttpServer" to smithyHttpServer,
                                    "Protocol" to protoInfo.markerStruct,
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

        // Generate the RoutingService construction with layer application for each protocol
        val routingServiceConstructions =
            writable {
                for (protoInfo in protocolInfos) {
                    val routerVarName = "${protoInfo.modulePath}_router"
                    val svcVarName = "${protoInfo.modulePath}_svc"
                    rustTemplate(
                        """
                        let $svcVarName = #{SmithyHttpServer}::routing::RoutingService::new($routerVarName);
                        let $svcVarName = $svcVarName.map(|s| s.layer(&self.layer));
                        """,
                        *codegenScope,
                    )
                }
            }

        // Generate the MultiProtocolService construction
        val multiProtocolConstruction =
            writable {
                rustTemplate(
                    "let router = #{SmithyHttpServer}::routing::MultiProtocolService::new()",
                    *codegenScope
                )
                for (protoInfo in protocolInfos) {
                    val svcVarName = "${protoInfo.modulePath}_svc"
                    rustTemplate(
                        """
                            .${protoInfo.builderMethod}($svcVarName)
                        """,
                        *codegenScope,
                    )
                }
                rust(";")
            }

        rustTemplate(
            """
            /// Constructs a [`$serviceName`] from the arguments provided to the builder.
            /// Operations without a handler default to returning 500 Internal Server Error to the caller.
            ///
            /// This service supports multiple protocols: ${protocolInfos.joinToString(", ") { it.modulePath }}.
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
                // Build router for each protocol
                #{RouterConstructions:W}

                // Wrap each router in RoutingService and apply user's layer
                #{RoutingServiceConstructions:W}

                // Combine into MultiProtocolService
                #{MultiProtocolConstruction:W}

                $serviceName { svc: router }
            }
            """,
            *codegenScope,
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
                // Single protocol: use the type alias as default
                rustTemplate(
                    """
                    ///
                    /// See the [root](crate) documentation for more information.
                    ##[derive(Clone)]
                    pub struct $serviceName<S = $routerName> {
                        // This is the router wrapped by layers.
                        svc: S,
                    }
                    """,
                    *codegenScope,
                )
            }

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
                "NotSetFields1" to notSetFields(),
                "NotSetFields2" to notSetFields(),
                *codegenScope,
            )

            // Protocol-specific layer and boxed methods only for single-protocol services
            if (!isMultiProtocol) {
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
                operationStructNames.map { (shape, name) ->
                    writable {
                        val absolute = shape.id.toString().replace("#", "##")
                        rustTemplate(
                            """
                            Operation::$name => #{SmithyHttpServer}::shape_id::ShapeId::new("$absolute", "${shape.id.namespace}", "${shape.id.name}"),
                            """,
                            *codegenScope,
                        )
                    }
                }.join("")
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
        writer.rustTemplate(
            """
            #{RouterTypeAlias:W}

            #{Builder:W}

            #{MissingOperationsError:W}

            #{RequestSpecs:W}

            #{Struct:W}

            #{Operations}

            #{ServiceImpl}
            """,
            "RouterTypeAlias" to routerTypeAlias(),
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
