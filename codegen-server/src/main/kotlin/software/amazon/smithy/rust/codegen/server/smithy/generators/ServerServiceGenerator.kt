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
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(runtimeConfig).toType()
    private val codegenScope =
        arrayOf(
            "Bytes" to RuntimeType.Bytes,
            "Http" to RuntimeType.Http,
            "SmithyHttp" to RuntimeType.smithyHttp(runtimeConfig),
            "HttpBody" to RuntimeType.HttpBody,
            "SmithyHttpServer" to smithyHttpServer,
            "Tower" to RuntimeType.Tower,
        )
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val crateName = codegenContext.moduleUseName()

    private val service = codegenContext.serviceShape
    private val serviceId = service.id
    private val serviceName = serviceId.name.toPascalCase()
    private val builderName = "${serviceName}Builder"
    private val builderBodyGenericTypeName = "Body"

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
            val spec = protocol.serverRouterRequestSpec(
                operationShape,
                operationName,
                serviceName,
                smithyHttpServer.resolve("routing::request_spec"),
            )
            val functionName = RustReservedWords.escapeIfNeeded(operationName.toSnakeCase())
            val functionBody = writable {
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

    /** A `Writable` block containing all the `Handler` and `Operation` setters for the builder. */
    private fun builderSetters(): Writable = writable {
        for ((operationShape, structName) in operationStructNames) {
            val fieldName = builderFieldNames[operationShape]
            val docHandler = DocHandlerGenerator(codegenContext, operationShape, "handler", "///")
            val handler = docHandler.docSignature()
            val handlerFixed = docHandler.docFixedSignature()
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
                /// use $crateName::$serviceName;
                ///
                #{HandlerImports:W}
                ///
                #{Handler:W}
                ///
                /// let app = $serviceName::builder_without_plugins()
                ///     .$fieldName(handler)
                ///     /* Set other handlers */
                ///     .build()
                ///     .unwrap();
                /// ## let app: $serviceName<#{SmithyHttpServer}::routing::Route<#{SmithyHttp}::body::SdkBody>> = app;
                /// ```
                ///
                pub fn $fieldName<HandlerType, HandlerExtractors, UpgradeExtractors>(self, handler: HandlerType) -> Self
                where
                    HandlerType: #{SmithyHttpServer}::operation::Handler<crate::operation_shape::$structName, HandlerExtractors>,

                    ModelPl: #{SmithyHttpServer}::plugin::Plugin<
                        $serviceName,
                        crate::operation_shape::$structName,
                        #{SmithyHttpServer}::operation::IntoService<crate::operation_shape::$structName, HandlerType>
                    >,
                    #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>: #{SmithyHttpServer}::plugin::Plugin<
                        $serviceName,
                        crate::operation_shape::$structName,
                        ModelPl::Output
                    >,
                    HttpPl: #{SmithyHttpServer}::plugin::Plugin<
                        $serviceName,
                        crate::operation_shape::$structName,
                        <
                            #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>
                            as #{SmithyHttpServer}::plugin::Plugin<
                                $serviceName,
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
                /// use $crateName::$serviceName;
                ///
                #{HandlerImports:W}
                ///
                #{HandlerFixed:W}
                ///
                /// let svc = #{Tower}::util::service_fn(handler);
                /// let app = $serviceName::builder_without_plugins()
                ///     .${fieldName}_service(svc)
                ///     /* Set other handlers */
                ///     .build()
                ///     .unwrap();
                /// ## let app: $serviceName<#{SmithyHttpServer}::routing::Route<#{SmithyHttp}::body::SdkBody>> = app;
                /// ```
                ///
                pub fn ${fieldName}_service<S, ServiceExtractors, UpgradeExtractors>(self, service: S) -> Self
                where
                    S: #{SmithyHttpServer}::operation::OperationService<crate::operation_shape::$structName, ServiceExtractors>,

                    ModelPl: #{SmithyHttpServer}::plugin::Plugin<
                        $serviceName,
                        crate::operation_shape::$structName,
                        #{SmithyHttpServer}::operation::Normalize<crate::operation_shape::$structName, S>
                    >,
                    #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>: #{SmithyHttpServer}::plugin::Plugin<
                        $serviceName,
                        crate::operation_shape::$structName,
                        ModelPl::Output
                    >,
                    HttpPl: #{SmithyHttpServer}::plugin::Plugin<
                        $serviceName,
                        crate::operation_shape::$structName,
                        <
                            #{SmithyHttpServer}::operation::UpgradePlugin::<UpgradeExtractors>
                            as #{SmithyHttpServer}::plugin::Plugin<
                                $serviceName,
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

    private fun buildMethod(): Writable = writable {
        val missingOperationsVariableName = "missing_operation_names"
        val expectMessageVariableName = "unexpected_error_msg"

        val nullabilityChecks = writable {
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
        val routesArrayElements = writable {
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
            /// unspecified route requested.
            pub fn build(self) -> Result<$serviceName<#{SmithyHttpServer}::routing::Route<$builderBodyGenericTypeName>>, MissingOperationsError>
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
                    let $expectMessageVariableName = "this should never panic since we are supposed to check beforehand that a handler has been registered for this operation; please file a bug report under https://github.com/awslabs/smithy-rs/issues";

                    #{PatternInitializations:W}

                    #{Router}::from_iter([#{RoutesArrayElements:W}])
                };
                Ok($serviceName {
                    router: #{SmithyHttpServer}::routing::RoutingService::new(router),
                })
            }
            """,
            "Router" to protocol.routerType(),
            "NullabilityChecks" to nullabilityChecks,
            "RoutesArrayElements" to routesArrayElements,
            "SmithyHttpServer" to smithyHttpServer,
            "PatternInitializations" to patternInitializations(),
        )
    }

    /**
     * Renders `PatternString::compile_regex()` function calls for every
     * `@pattern`-constrained string shape in the service closure.
     */
    @Suppress("DEPRECATION")
    private fun patternInitializations(): Writable {
        val patterns = Walker(model).walkShapes(service)
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

    private fun buildUncheckedMethod(): Writable = writable {
        val pairs = writable {
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
            pub fn build_unchecked(self) -> $serviceName<#{SmithyHttpServer}::routing::Route<$builderBodyGenericTypeName>>
            where
                $builderBodyGenericTypeName: Send + 'static
            {
                let router = #{Router}::from_iter([#{Pairs:W}]);
                $serviceName {
                    router: #{SmithyHttpServer}::routing::RoutingService::new(router),
                }
            }
            """,
            "Router" to protocol.routerType(),
            "Pairs" to pairs,
            "SmithyHttpServer" to smithyHttpServer,
        )
    }

    /** Returns a `Writable` containing the builder struct definition and its implementations. */
    private fun builder(): Writable = writable {
        val builderGenerics = listOf(builderBodyGenericTypeName, "HttpPl", "ModelPl").joinToString(", ")
        rustTemplate(
            """
            /// The service builder for [`$serviceName`].
            ///
            /// Constructed via [`$serviceName::builder_with_plugins`] or [`$serviceName::builder_without_plugins`].
            pub struct $builderName<$builderGenerics> {
                ${builderFields.joinToString(", ")},
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

    private fun requestSpecsModule(): Writable = writable {
        val functions = writable {
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

    /** Returns a `Writable` comma delimited sequence of `builder_field: None`. */
    private val notSetFields = builderFieldNames.values.map {
        writable {
            rustTemplate(
                "$it: None",
                *codegenScope,
            )
        }
    }

    /** Returns a `Writable` containing the service struct definition and its implementations. */
    private fun serviceStruct(): Writable = writable {
        documentShape(service, model)

        rustTemplate(
            """
            ///
            /// See the [root](crate) documentation for more information.
            ##[derive(Clone)]
            pub struct $serviceName<S = #{SmithyHttpServer}::routing::Route> {
                router: #{SmithyHttpServer}::routing::RoutingService<#{Router}<S>, #{Protocol}>,
            }

            impl $serviceName<()> {
                /// Constructs a builder for [`$serviceName`].
                /// You must specify what plugins should be applied to the operations in this service.
                ///
                /// Use [`$serviceName::builder_without_plugins`] if you don't need to apply plugins.
                ///
                /// Check out [`HttpPlugins`](#{SmithyHttpServer}::plugin::HttpPlugins) and
                /// [`ModelPlugins`](#{SmithyHttpServer}::plugin::ModelPlugins) if you need to apply
                /// multiple plugins.
                pub fn builder_with_plugins<Body, HttpPl: #{SmithyHttpServer}::plugin::HttpMarker, ModelPl: #{SmithyHttpServer}::plugin::ModelMarker>(http_plugin: HttpPl, model_plugin: ModelPl) -> $builderName<Body, HttpPl, ModelPl> {
                    $builderName {
                        #{NotSetFields:W},
                        http_plugin,
                        model_plugin
                    }
                }

                /// Constructs a builder for [`$serviceName`].
                ///
                /// Use [`$serviceName::builder_with_plugins`] if you need to specify plugins.
                pub fn builder_without_plugins<Body>() -> $builderName<Body, #{SmithyHttpServer}::plugin::IdentityPlugin, #{SmithyHttpServer}::plugin::IdentityPlugin> {
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

                /// Applies a [`Layer`](#{Tower}::Layer) uniformly to all routes.
                pub fn layer<L>(self, layer: &L) -> $serviceName<L::Service>
                where
                    L: #{Tower}::Layer<S>
                {
                    $serviceName {
                        router: self.router.map(|s| s.layer(layer))
                    }
                }

                /// Applies [`Route::new`](#{SmithyHttpServer}::routing::Route::new) to all routes.
                ///
                /// This has the effect of erasing all types accumulated via [`layer`]($serviceName::layer).
                pub fn boxed<B>(self) -> $serviceName<#{SmithyHttpServer}::routing::Route<B>>
                where
                    S: #{Tower}::Service<
                        #{Http}::Request<B>,
                        Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>,
                        Error = std::convert::Infallible>,
                    S: Clone + Send + 'static,
                    S::Future: Send + 'static,
                {
                    self.layer(&#{Tower}::layer::layer_fn(#{SmithyHttpServer}::routing::Route::new))
                }
            }

            impl<B, RespB, S> #{Tower}::Service<#{Http}::Request<B>> for $serviceName<S>
            where
                S: #{Tower}::Service<#{Http}::Request<B>, Response = #{Http}::Response<RespB>> + Clone,
                RespB: #{HttpBody}::Body<Data = #{Bytes}> + Send + 'static,
                RespB::Error: Into<Box<dyn std::error::Error + Send + Sync>>
            {
                type Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>;
                type Error = S::Error;
                type Future = #{SmithyHttpServer}::routing::RoutingFuture<S, B>;

                fn poll_ready(&mut self, cx: &mut std::task::Context) -> std::task::Poll<Result<(), Self::Error>> {
                    self.router.poll_ready(cx)
                }

                fn call(&mut self, request: #{Http}::Request<B>) -> Self::Future {
                    self.router.call(request)
                }
            }
            """,
            "NotSetFields" to notSetFields.join(", "),
            "Router" to protocol.routerType(),
            "Protocol" to protocol.markerStruct(),
            *codegenScope,
        )
    }

    private fun missingOperationsError(): Writable = writable {
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

    private fun serviceShapeImpl(): Writable = writable {
        val namespace = serviceId.namespace
        val name = serviceId.name
        val absolute = serviceId.toString().replace("#", "##")
        val version = codegenContext.serviceShape.version?.let { "Some(\"$it\")" } ?: "None"
        rustTemplate(
            """
            impl #{SmithyHttpServer}::service::ServiceShape for $serviceName {
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

    private fun operationEnum(): Writable = writable {
        val operations = operationStructNames.values.joinToString(",")
        val matchArms: Writable = operationStructNames.map {
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
                impl #{SmithyHttpServer}::service::ContainsOperation<crate::operation_shape::$value> for $serviceName {
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

/**
 * Returns a writable to import the necessary modules used by a handler implementation stub.
 *
 * ```rust
 * use my_service::{input, output, error};
 * ```
 */
fun handlerImports(crateName: String, operations: Collection<OperationShape>, commentToken: String = "///") = writable {
    val hasErrors = operations.any { it.errors.isNotEmpty() }
    val errorImport = if (hasErrors) ", ${ErrorModule.name}" else ""
    if (operations.isNotEmpty()) {
        rust("$commentToken use $crateName::{${InputModule.name}, ${OutputModule.name}$errorImport};")
    }
}
