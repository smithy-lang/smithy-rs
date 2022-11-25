/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.ErrorsModule
import software.amazon.smithy.rust.codegen.core.smithy.InputsModule
import software.amazon.smithy.rust.codegen.core.smithy.OutputsModule
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol

class ServerServiceGeneratorV2(
    private val codegenContext: CodegenContext,
    private val protocol: ServerProtocol,
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val smithyHttpServer = ServerCargoDependency.SmithyHttpServer(runtimeConfig).toType()
    private val codegenScope =
        arrayOf(
            "Bytes" to CargoDependency.Bytes.toType(),
            "Http" to CargoDependency.Http.toType(),
            "SmithyHttp" to CargoDependency.smithyHttp(runtimeConfig).toType(),
            "HttpBody" to CargoDependency.HttpBody.toType(),
            "SmithyHttpServer" to smithyHttpServer,
            "Tower" to CargoDependency.Tower.toType(),
        )
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    val crateName = codegenContext.settings.moduleName.toSnakeCase()

    private val service = codegenContext.serviceShape
    private val serviceName = service.id.name.toPascalCase()
    private val builderName = "${serviceName}Builder"
    private val builderPluginGenericTypeName = "Plugin"
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
                smithyHttpServer.member("routing::request_spec"),
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
                    "SpecType" to protocol.serverRouterRequestSpecType(smithyHttpServer.member("routing::request_spec")),
                )
            }
            Pair(functionName, functionBody)
        }

    /** A `Writable` block containing all the `Handler` and `Operation` setters for the builder. */
    private fun builderSetters(): Writable = writable {
        for ((operationShape, structName) in operationStructNames) {
            val fieldName = builderFieldNames[operationShape]
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
                pub fn $fieldName<HandlerType, Extensions>(self, handler: HandlerType) -> Self
                where
                    HandlerType: #{SmithyHttpServer}::operation::Handler<crate::operation_shape::$structName, Extensions>,
                    #{SmithyHttpServer}::operation::Operation<#{SmithyHttpServer}::operation::IntoService<crate::operation_shape::$structName, HandlerType>>:
                        #{SmithyHttpServer}::operation::Upgradable<
                            #{Protocol},
                            crate::operation_shape::$structName,
                            Extensions,
                            $builderBodyGenericTypeName,
                            $builderPluginGenericTypeName,
                        >
                {
                    use #{SmithyHttpServer}::operation::OperationShapeExt;
                    self.${fieldName}_operation(crate::operation_shape::$structName::from_handler(handler))
                }

                /// Sets the [`$structName`](crate::operation_shape::$structName) operation.
                ///
                /// This should be an [`Operation`](#{SmithyHttpServer}::operation::Operation) created from
                /// [`$structName`](crate::operation_shape::$structName) using either
                /// [`OperationShape::from_handler`](#{SmithyHttpServer}::operation::OperationShapeExt::from_handler) or
                /// [`OperationShape::from_service`](#{SmithyHttpServer}::operation::OperationShapeExt::from_service).
                pub fn ${fieldName}_operation<Operation, Extensions>(mut self, operation: Operation) -> Self
                where
                    Operation: #{SmithyHttpServer}::operation::Upgradable<
                        #{Protocol},
                        crate::operation_shape::$structName,
                        Extensions,
                        $builderBodyGenericTypeName,
                        $builderPluginGenericTypeName,
                    >
                {
                    self.$fieldName = Some(operation.upgrade(&self.plugin));
                    self
                }
                """,
                "Protocol" to protocol.markerStruct(),
                "Handler" to DocHandlerGenerator(operationShape, "///", "handler", codegenContext)::render,
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
                        $missingOperationsVariableName.insert(crate::operation_shape::$operationZstTypeName::NAME, ".$fieldName()");
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
                    #{Router}::from_iter([#{RoutesArrayElements:W}])
                };
                Ok($serviceName {
                    router: #{SmithyHttpServer}::routers::RoutingService::new(router),
                })
            }
            """,
            "Router" to protocol.routerType(),
            "NullabilityChecks" to nullabilityChecks,
            "RoutesArrayElements" to routesArrayElements,
            "SmithyHttpServer" to smithyHttpServer,
        )
    }

    private fun buildUncheckedMethod(): Writable = writable {
        val pairs = writable {
            for (operationShape in operations) {
                val fieldName = builderFieldNames[operationShape]!!
                val (specBuilderFunctionName, _) = requestSpecMap.getValue(operationShape)
                val operationZstTypeName = operationStructNames[operationShape]!!
                rustTemplate(
                    """
                    (
                        $requestSpecsModuleName::$specBuilderFunctionName(),
                        self.$fieldName.unwrap_or_else(|| {
                            #{SmithyHttpServer}::routing::Route::new(<#{SmithyHttpServer}::operation::FailOnMissingOperation as #{SmithyHttpServer}::operation::Upgradable<
                                #{Protocol},
                                crate::operation_shape::$operationZstTypeName,
                                (),
                                _,
                                _,
                            >>::upgrade(#{SmithyHttpServer}::operation::FailOnMissingOperation, &self.plugin))
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
            /// Operations without a handler default to returning 500s to the caller.
            ///
            /// Check out [`$builderName::build`] if you'd prefer the builder to fail if one or more operations do
            /// not have a registered handler.
            pub fn build_unchecked(self) -> $serviceName<#{SmithyHttpServer}::routing::Route<$builderBodyGenericTypeName>>
            where
                $builderBodyGenericTypeName: Send + 'static
            {
                let router = #{Router}::from_iter([#{Pairs:W}]);
                $serviceName {
                    router: #{SmithyHttpServer}::routers::RoutingService::new(router),
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
        val builderGenerics = listOf(builderBodyGenericTypeName, builderPluginGenericTypeName).joinToString(", ")
        rustTemplate(
            """
            /// The service builder for [`$serviceName`].
            ///
            /// Constructed via [`$serviceName::builder_with_plugins`] or [`$serviceName::builder_without_plugins`].
            pub struct $builderName<$builderGenerics> {
                ${builderFields.joinToString(", ")},
                plugin: $builderPluginGenericTypeName,
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
            ##[derive(Clone)]
            pub struct $serviceName<S = #{SmithyHttpServer}::routing::Route> {
                router: #{SmithyHttpServer}::routers::RoutingService<#{Router}<S>, #{Protocol}>,
            }

            impl $serviceName<()> {
                /// Constructs a builder for [`$serviceName`].
                /// You must specify what plugins should be applied to the operations in this service.
                ///
                /// Use [`$serviceName::builder_without_plugins`] if you don't need to apply plugins.
                ///
                /// Check out [`PluginPipeline`](#{SmithyHttpServer}::plugin::PluginPipeline) if you need to apply
                /// multiple plugins.
                pub fn builder_with_plugins<Body, Plugin>(plugin: Plugin) -> $builderName<Body, Plugin> {
                    $builderName {
                        #{NotSetFields:W},
                        plugin
                    }
                }

                /// Constructs a builder for [`$serviceName`].
                ///
                /// Use [`$serviceName::builder_with_plugins`] if you need to specify plugins.
                pub fn builder_without_plugins<Body>() -> $builderName<Body, #{SmithyHttpServer}::plugin::IdentityPlugin> {
                    Self::builder_with_plugins(#{SmithyHttpServer}::plugin::IdentityPlugin)
                }
            }

            impl<S> $serviceName<S> {
                /// Converts [`$serviceName`] into a [`MakeService`](tower::make::MakeService).
                pub fn into_make_service(self) -> #{SmithyHttpServer}::routing::IntoMakeService<Self> {
                    #{SmithyHttpServer}::routing::IntoMakeService::new(self)
                }

                /// Converts [`$serviceName`] into a [`MakeService`](tower::make::MakeService) with [`ConnectInfo`](#{SmithyHttpServer}::request::connect_info::ConnectInfo).
                pub fn into_make_service_with_connect_info<C>(self) -> #{SmithyHttpServer}::request::connect_info::IntoMakeServiceWithConnectInfo<Self, C> {
                    #{SmithyHttpServer}::request::connect_info::IntoMakeServiceWithConnectInfo::new(self)
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
                /// This has the effect of erasing all types accumulated via [`layer`].
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
                RespB: #{HttpBody}::Body<Data = #{Bytes}::Bytes> + Send + 'static,
                RespB::Error: Into<Box<dyn std::error::Error + Send + Sync>>
            {
                type Response = #{Http}::Response<#{SmithyHttpServer}::body::BoxBody>;
                type Error = S::Error;
                type Future = #{SmithyHttpServer}::routers::RoutingFuture<S, B>;

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
        rust(
            """
            /// The error encountered when calling the [`$builderName::build`] method if one or more operation handlers are not
            /// specified.
            ##[derive(Debug)]
            pub struct MissingOperationsError {
                operation_names2setter_methods: std::collections::HashMap<&'static str, &'static str>,
            }

            impl std::fmt::Display for MissingOperationsError {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(
                        f,
                        "You must specify a handler for all operations attached to `$serviceName`.\n\
                        We are missing handlers for the following operations:\n",
                    )?;
                    for operation_name in self.operation_names2setter_methods.keys() {
                        writeln!(f, "- {}", operation_name)?;
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
        )
    }

    fun render(writer: RustWriter) {
        val crateName = codegenContext.moduleUseName()
        val handlers: Writable = operations
            .map { operation ->
                DocHandlerGenerator(operation, "///", builderFieldNames[operation]!!, codegenContext).docSignature()
            }
            .reduce { acc, wt ->
                writable {
                    rustTemplate("#{acc:W} \n#{wt:W}", "acc" to acc, "wt" to wt)
                }
            }

        val hasErrors = service.operations.any { model.expectShape(it).asOperationShape().get().errors.isNotEmpty() }

        writer.rustTemplate(
            """
            /// A fast and customizable Rust implementation of the $serviceName Smithy service.
            ///
            /// The primary export is [`$serviceName`]: it satisfies the [`Service<http::Request, Response = http::Response>`]
            /// trait and therefore can be handed to [Hyper server] using [`$serviceName::into_make_service`] or used in Lambda using [`#{SmithyHttpServer}::routing::LambdaHandler`].
            /// The [`crate::${InputsModule.name}`], ${if (!hasErrors) "and " else ""}[`crate::${OutputsModule.name}`], ${if (hasErrors) "and [`crate::${ErrorsModule.name}`]" else "" }
            /// modules provide the types used in each operation.
            ///
            /// The primary export is [`$serviceName`]: it is the
            /// [`$builderName`] is used to set your business logic to implement your [operations],
            /// customize your [operations]'s behaviors by applying middleware,
            /// and build your service.
            ///
            /// [`$builderName`] contains the [operations] modeled in your Smithy service.
            /// You must set an implementation for all operations with the correct signature,
            /// or your service will fail to be constructed at runtime and panic.
            /// For each of the [operations] modeled in
            /// your Smithy service, you need to provide an implementation in the
            /// form of an async function that takes in the
            /// operation's input as their first parameter, and returns the
            /// operation's output. If your operation is fallible (i.e. it
            /// contains the `errors` member in your Smithy model), the function
            /// implementing the operation has to be fallible (i.e. return a [`Result`]).
            /// The possible forms for your async functions are:
            /// ```rust,no_run
            /// async fn handler_fallible(input: Input, extensions: #{SmithyHttpServer}::Extension<T>) -> Result<Output, Error>
            /// async fn handler_infallible(input: Input, extensions: #{SmithyHttpServer}::Extension<T>) -> Output
            /// ```
            /// Both can take up to 8 extensions, or none:
            /// ```rust,no_run
            /// async fn handler_with_no_extensions(input: Input) -> ...;
            /// async fn handler_with_one_extension(input: Input, ext: #{SmithyHttpServer}::Extension<T>) -> ...
            /// async fn handler_with_two_extensions(input: Input, ext0: #{SmithyHttpServer}::Extension<T>, ext1: #{SmithyHttpServer}::Extension<T>) -> ...
            /// ...
            /// ```
            /// For a full list of the possible extensions, see: [`#{SmithyHttpServer}::request`]. Any `T: Send + Sync + 'static` is also allowed.
            ///
            /// To construct [`$serviceName`], you can use:
            /// * [`$serviceName::builder_without_plugins`] which returns a [`$builderName`] without any service-wide middleware applied.
            /// * [`$serviceName::builder_with_plugins`] which returns a [`$builderName`] that applies `plugins` to all your operations.
            ///
            /// To know more about plugins, see: [`#{SmithyHttpServer}::plugin`].
            ///
            /// When you have set all your operations, you can convert [`$serviceName`] into a tower [Service] calling:
            /// * [`$serviceName::into_make_service`] that converts $serviceName into a type that implements [`tower::make::MakeService`], a _service factory_.
            /// * [`$serviceName::into_make_service_with_connect_info`] that converts $serviceName into [`tower::make::MakeService`]
            /// with [`ConnectInfo`](#{SmithyHttpServer}::request::connect_info::ConnectInfo).
            /// You can write your implementations to be passed in the connection information, populated by the [Hyper server], as an [`#{SmithyHttpServer}::Extension`].
            ///
            /// You can feed this [Service] to a [Hyper server], and the
            /// server will instantiate and [`serve`] your service.
            ///
            /// Here's a full example to get you started:
            ///
            /// ```rust
            /// use std::net::SocketAddr;
            /// use $crateName::$serviceName;
            ///
            /// ##[tokio::main]
            /// pub async fn main() {
            ///    let app = $serviceName::builder_without_plugins()
            ${builderFieldNames.values.joinToString("\n") { "///        .$it($it)" }}
            ///        .build()
            ///        .expect("failed to build an instance of $serviceName");
            ///
            ///    let bind: SocketAddr = "127.0.0.1:6969".parse()
            ///        .expect("unable to parse the server bind address and port");
            ///
            ///    let server = hyper::Server::bind(&bind).serve(app.into_make_service());
            ///    ## let server = async { Ok::<_, ()>(()) };
            ///
            ///    // Run your service!
            ///    if let Err(err) = server.await {
            ///      eprintln!("server error: {}", err);
            ///    }
            /// }
            ///
            #{Handlers:W}
            ///
            /// ```
            ///
            /// [`serve`]: https://docs.rs/hyper/0.14.16/hyper/server/struct.Builder.html##method.serve
            /// [`tower::make::MakeService`]: https://docs.rs/tower/latest/tower/make/trait.MakeService.html
            /// [HTTP binding traits]: https://smithy.io/2.0/spec/http-bindings.html
            /// [operations]: https://smithy.io/2.0/spec/service-types.html##operation
            /// [Hyper server]: https://docs.rs/hyper/latest/hyper/server/index.html
            /// [Service]: https://docs.rs/tower-service/latest/tower_service/trait.Service.html

            #{Builder:W}

            #{MissingOperationsError:W}

            #{RequestSpecs:W}

            #{Struct:W}
            """,
            "Builder" to builder(),
            "MissingOperationsError" to missingOperationsError(),
            "RequestSpecs" to requestSpecsModule(),
            "Struct" to serviceStruct(),
            "Handlers" to handlers,
            "ExampleHandler" to operations.take(1).map { operation -> DocHandlerGenerator(operation, "///", builderFieldNames[operation]!!, codegenContext).docSignature() },
            *codegenScope,
        )
    }
}
