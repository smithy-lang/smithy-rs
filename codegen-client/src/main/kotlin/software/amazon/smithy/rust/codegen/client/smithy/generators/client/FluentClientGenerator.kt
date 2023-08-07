/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.PaginatorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.isPaginated
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.EscapeFor
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.asArgumentType
import software.amazon.smithy.rust.codegen.core.rustlang.asOptional
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docLink
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.normalizeHtml
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTypeParameters
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.getterName
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class FluentClientGenerator(
    private val codegenContext: ClientCodegenContext,
    private val reexportSmithyClientBuilder: Boolean = true,
    private val generics: FluentClientGenerics,
    private val customizations: List<FluentClientCustomization> = emptyList(),
    private val retryClassifier: RuntimeType = RuntimeType.smithyHttp(codegenContext.runtimeConfig)
        .resolve("retry::DefaultResponseRetryClassifier"),
) {
    companion object {
        fun clientOperationFnName(operationShape: OperationShape, symbolProvider: RustSymbolProvider): String =
            RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operationShape).name.toSnakeCase())

        fun clientOperationModuleName(operationShape: OperationShape, symbolProvider: RustSymbolProvider): String =
            RustReservedWords.escapeIfNeeded(
                symbolProvider.toSymbol(operationShape).name.toSnakeCase(),
                EscapeFor.ModuleName,
            )
    }

    private val serviceShape = codegenContext.serviceShape
    private val operations =
        TopDownIndex.of(codegenContext.model).getContainedOperations(serviceShape).sortedBy { it.id }
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val core = FluentClientCore(model)
    private val smithyRuntimeMode = codegenContext.smithyRuntimeMode

    fun render(crate: RustCrate, customizableOperationCustomizations: List<CustomizableOperationCustomization> = emptyList()) {
        renderFluentClient(crate)

        val customizableOperationGenerator = CustomizableOperationGenerator(codegenContext, generics, customizableOperationCustomizations)
        operations.forEach { operation ->
            crate.withModule(symbolProvider.moduleForBuilder(operation)) {
                renderFluentBuilder(operation)
            }
        }

        customizableOperationGenerator.render(crate)
        if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
            customizableOperationGenerator.renderForOrchestrator(crate)
        }
    }

    private fun renderFluentClient(crate: RustCrate) {
        crate.withModule(ClientRustModule.client) {
            if (reexportSmithyClientBuilder) {
                rustTemplate(
                    """
                    ##[doc(inline)]
                    pub use #{client}::Builder;
                    """,
                    "client" to RuntimeType.smithyClient(runtimeConfig),
                )
            }
            val clientScope = arrayOf(
                *preludeScope,
                "Arc" to RuntimeType.Arc,
                "client" to RuntimeType.smithyClient(runtimeConfig),
                "client_docs" to writable
                    {
                        customizations.forEach {
                            it.section(
                                FluentClientSection.FluentClientDocs(
                                    serviceShape,
                                ),
                            )(this)
                        }
                    },
                "RetryConfig" to RuntimeType.smithyTypes(runtimeConfig).resolve("retry::RetryConfig"),
                "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
                "TimeoutConfig" to RuntimeType.smithyTypes(runtimeConfig).resolve("timeout::TimeoutConfig"),
                // TODO(enableNewSmithyRuntimeCleanup): Delete the generics when cleaning up middleware
                "generics_decl" to generics.decl,
                "smithy_inst" to generics.smithyInst,
            )
            if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                rustTemplate(
                    """
                    ##[derive(Debug)]
                    pub(crate) struct Handle#{generics_decl:W} {
                        pub(crate) client: #{client}::Client#{smithy_inst:W},
                        pub(crate) conf: crate::Config,
                    }

                    #{client_docs:W}
                    ##[derive(::std::fmt::Debug)]
                    pub struct Client#{generics_decl:W} {
                        handle: #{Arc}<Handle${generics.inst}>
                    }

                    impl${generics.inst} #{Clone} for Client${generics.inst} {
                        fn clone(&self) -> Self {
                            Self { handle: self.handle.clone() }
                        }
                    }

                    impl${generics.inst} From<#{client}::Client#{smithy_inst:W}> for Client${generics.inst} {
                        fn from(client: #{client}::Client#{smithy_inst:W}) -> Self {
                            Self::with_config(client, crate::Config::builder().build())
                        }
                    }

                    impl${generics.inst} Client${generics.inst} {
                        /// Creates a client with the given service configuration.
                        pub fn with_config(client: #{client}::Client#{smithy_inst:W}, conf: crate::Config) -> Self {
                            Self {
                                handle: #{Arc}::new(Handle {
                                    client,
                                    conf,
                                })
                            }
                        }

                        /// Returns the client's configuration.
                        pub fn conf(&self) -> &crate::Config {
                            &self.handle.conf
                        }
                    }
                    """,
                    *clientScope,
                )
            } else {
                rustTemplate(
                    """
                    ##[derive(Debug)]
                    pub(crate) struct Handle {
                        pub(crate) conf: crate::Config,
                        pub(crate) runtime_plugins: #{RuntimePlugins},
                    }

                    #{client_docs:W}
                    ##[derive(#{Clone}, ::std::fmt::Debug)]
                    pub struct Client {
                        handle: #{Arc}<Handle>,
                    }

                    impl Client {
                        /// Creates a new client from the service [`Config`](crate::Config).
                        ///
                        /// ## Panics
                        ///
                        /// - This method will panic if the `conf` is missing an async sleep implementation. If you experience this panic, set
                        ///     the `sleep_impl` on the Config passed into this function to fix it.
                        /// - This method will panic if the `conf` is missing an HTTP connector. If you experience this panic, set the
                        ///     `http_connector` on the Config passed into this function to fix it.
                        pub fn from_conf(conf: crate::Config) -> Self {
                            let retry_config = conf.retry_config().cloned().unwrap_or_else(#{RetryConfig}::disabled);
                            let timeout_config = conf.timeout_config().cloned().unwrap_or_else(#{TimeoutConfig}::disabled);
                            let sleep_impl = conf.sleep_impl();
                            if (retry_config.has_retry() || timeout_config.has_timeouts()) && sleep_impl.is_none() {
                                panic!("An async sleep implementation is required for retries or timeouts to work. \
                                        Set the `sleep_impl` on the Config passed into this function to fix this panic.");
                            }

                            Self {
                                handle: #{Arc}::new(
                                    Handle {
                                        conf: conf.clone(),
                                        runtime_plugins: #{base_client_runtime_plugins}(conf),
                                    }
                                )
                            }
                        }

                        /// Returns the client's configuration.
                        pub fn config(&self) -> &crate::Config {
                            &self.handle.conf
                        }

                        ##[doc(hidden)]
                        // TODO(enableNewSmithyRuntimeCleanup): Delete this function when cleaning up middleware
                        // This is currently kept around so the tests still compile in both modes
                        /// Creates a client with the given service configuration.
                        pub fn with_config<C, M, R>(_client: #{client}::Client<C, M, R>, conf: crate::Config) -> Self {
                            Self::from_conf(conf)
                        }

                        ##[doc(hidden)]
                        // TODO(enableNewSmithyRuntimeCleanup): Delete this function when cleaning up middleware
                        // This is currently kept around so the tests still compile in both modes
                        /// Returns the client's configuration.
                        pub fn conf(&self) -> &crate::Config {
                            &self.handle.conf
                        }
                    }
                    """,
                    *clientScope,
                    "base_client_runtime_plugins" to baseClientRuntimePluginsFn(runtimeConfig),
                )
            }
        }

        operations.forEach { operation ->
            val name = symbolProvider.toSymbol(operation).name
            val fnName = clientOperationFnName(operation, symbolProvider)
            val moduleName = clientOperationModuleName(operation, symbolProvider)

            val privateModule = RustModule.private(moduleName, parent = ClientRustModule.client)
            crate.withModule(privateModule) {
                rustBlockTemplate(
                    "impl${generics.inst} super::Client${generics.inst} #{bounds:W}",
                    "client" to RuntimeType.smithyClient(runtimeConfig),
                    "bounds" to generics.bounds,
                ) {
                    val fullPath = operation.fullyQualifiedFluentBuilder(symbolProvider)
                    val maybePaginated = if (operation.isPaginated(model)) {
                        "\n/// This operation supports pagination; See [`into_paginator()`]($fullPath::into_paginator)."
                    } else {
                        ""
                    }

                    val output = operation.outputShape(model)
                    val operationOk = symbolProvider.toSymbol(output)
                    val operationErr = symbolProvider.symbolForOperationError(operation)

                    val inputFieldsBody = generateOperationShapeDocs(this, symbolProvider, operation, model)
                        .joinToString("\n") { "///   - $it" }

                    val inputFieldsHead = if (inputFieldsBody.isNotEmpty()) {
                        "The fluent builder is configurable:\n"
                    } else {
                        "The fluent builder takes no input, just [`send`]($fullPath::send) it."
                    }

                    val outputFieldsBody =
                        generateShapeMemberDocs(this, symbolProvider, output, model).joinToString("\n") {
                            "///   - $it"
                        }

                    var outputFieldsHead = "On success, responds with [`${operationOk.name}`]($operationOk)"
                    if (outputFieldsBody.isNotEmpty()) {
                        outputFieldsHead += " with field(s):\n"
                    }

                    rustTemplate(
                        """
                        /// Constructs a fluent builder for the [`$name`]($fullPath) operation.$maybePaginated
                        ///
                        /// - $inputFieldsHead$inputFieldsBody
                        /// - $outputFieldsHead$outputFieldsBody
                        /// - On failure, responds with [`SdkError<${operationErr.name}>`]($operationErr)
                        """,
                    )

                    // Write a deprecation notice if this operation is deprecated.
                    deprecatedShape(operation)

                    rustTemplate(
                        """
                        pub fn $fnName(&self) -> #{FluentBuilder}${generics.inst} {
                            #{FluentBuilder}::new(self.handle.clone())
                        }
                        """,
                        "FluentBuilder" to operation.fluentBuilderType(symbolProvider),
                    )
                }
            }
        }
    }

    private fun RustWriter.renderFluentBuilder(operation: OperationShape) {
        val outputType = symbolProvider.toSymbol(operation.outputShape(model))
        val errorType = symbolProvider.symbolForOperationError(operation)
        val operationSymbol = symbolProvider.toSymbol(operation)

        val input = operation.inputShape(model)
        val baseDerives = symbolProvider.toSymbol(input).expectRustMetadata().derives
        // Filter out any derive that isn't Clone. Then add a Debug derive
        // input name
        val fnName = clientOperationFnName(operation, symbolProvider)
        implBlock(symbolProvider.symbolForBuilder(input)) {
            rustTemplate(
                """
                /// Sends a request with this input using the given client.
                pub async fn send_with${generics.inst}(
                    self,
                    client: &crate::Client${generics.inst}
                ) -> #{Result}<
                    #{OperationOutput},
                    #{SdkError}<
                        #{OperationError},
                        #{RawResponseType}
                    >
                > #{send_bounds:W} #{boundsWithoutWhereClause:W} {
                    let mut fluent_builder = client.$fnName();
                    fluent_builder.inner = self;
                    fluent_builder.send().await
                }
                """,
                *preludeScope,
                "RawResponseType" to if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                    RuntimeType.smithyHttp(runtimeConfig).resolve("operation::Response")
                } else {
                    RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::orchestrator::HttpResponse")
                },
                "Operation" to operationSymbol,
                "OperationError" to errorType,
                "OperationOutput" to outputType,
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
                "SdkSuccess" to RuntimeType.sdkSuccess(runtimeConfig),
                "boundsWithoutWhereClause" to generics.boundsWithoutWhereClause,
                "send_bounds" to generics.sendBounds(operationSymbol, outputType, errorType, retryClassifier),
            )
        }

        val derives = baseDerives.filter { it == RuntimeType.Clone } + RuntimeType.Debug
        docs("Fluent builder constructing a request to `${operationSymbol.name}`.\n")

        val builderName = operation.fluentBuilderType(symbolProvider).name
        documentShape(operation, model, autoSuppressMissingDocs = false)
        deprecatedShape(operation)
        Attribute(derive(derives.toSet())).render(this)
        withBlockTemplate(
            "pub struct $builderName#{generics:W} {",
            "}",
            "generics" to generics.decl,
        ) {
            rustTemplate(
                """
                handle: #{Arc}<crate::client::Handle${generics.inst}>,
                inner: #{Inner},
                """,
                "Inner" to symbolProvider.symbolForBuilder(input),
                "Arc" to RuntimeType.Arc,
                "generics" to generics.decl,
            )
            if (smithyRuntimeMode.generateOrchestrator) {
                rustTemplate("config_override: #{Option}<crate::config::Builder>,", *preludeScope)
            }
        }

        rustBlockTemplate(
            "impl${generics.inst} $builderName${generics.inst} #{bounds:W}",
            "client" to RuntimeType.smithyClient(runtimeConfig),
            "bounds" to generics.bounds,
        ) {
            rust("/// Creates a new `${operationSymbol.name}`.")
            withBlockTemplate(
                "pub(crate) fn new(handle: #{Arc}<crate::client::Handle${generics.inst}>) -> Self {",
                "}",
                "Arc" to RuntimeType.Arc,
                "generics" to generics.decl,
            ) {
                withBlockTemplate(
                    "Self {",
                    "}",
                ) {
                    rustTemplate("handle, inner: #{Default}::default(),", *preludeScope)
                    if (smithyRuntimeMode.generateOrchestrator) {
                        rustTemplate("config_override: #{None},", *preludeScope)
                    }
                }
            }

            rust("/// Access the ${operationSymbol.name} as a reference.\n")
            withBlockTemplate(
                "pub fn as_input(&self) -> &#{Inner} {", "}",
                "Inner" to symbolProvider.symbolForBuilder(input),
            ) {
                write("&self.inner")
            }

            if (smithyRuntimeMode.generateMiddleware) {
                val middlewareScope = arrayOf(
                    *preludeScope,
                    "CustomizableOperation" to ClientRustModule.Client.customize.toType()
                        .resolve("CustomizableOperation"),
                    "ClassifyRetry" to RuntimeType.classifyRetry(runtimeConfig),
                    "Operation" to operationSymbol,
                    "OperationError" to errorType,
                    "OperationOutput" to outputType,
                    "SdkError" to RuntimeType.sdkError(runtimeConfig),
                    "SdkSuccess" to RuntimeType.sdkSuccess(runtimeConfig),
                    "send_bounds" to generics.sendBounds(operationSymbol, outputType, errorType, retryClassifier),
                    "customizable_op_type_params" to rustTypeParameters(
                        symbolProvider.toSymbol(operation),
                        retryClassifier,
                        generics.toRustGenerics(),
                    ),
                )
                if (smithyRuntimeMode.generateMiddleware) {
                    rustTemplate(
                        """
                        /// Sends the request and returns the response.
                        ///
                        /// If an error occurs, an `SdkError` will be returned with additional details that
                        /// can be matched against.
                        ///
                        /// By default, any retryable failures will be retried twice. Retry behavior
                        /// is configurable with the [RetryConfig](aws_smithy_types::retry::RetryConfig), which can be
                        /// set when configuring the client.
                        pub async fn send(self) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}>>
                        #{send_bounds:W} {
                            let op = self.inner.build().map_err(#{SdkError}::construction_failure)?
                                .make_operation(&self.handle.conf)
                                .await
                                .map_err(#{SdkError}::construction_failure)?;
                            self.handle.client.call(op).await
                        }

                        /// Consumes this builder, creating a customizable operation that can be modified before being
                        /// sent. The operation's inner [http::Request] can be modified as well.
                        pub async fn customize(self) -> #{Result}<
                            #{CustomizableOperation}#{customizable_op_type_params:W},
                            #{SdkError}<#{OperationError}>
                        > #{send_bounds:W} {
                            let handle = self.handle.clone();
                            let operation = self.inner.build().map_err(#{SdkError}::construction_failure)?
                                .make_operation(&handle.conf)
                                .await
                                .map_err(#{SdkError}::construction_failure)?;
                            #{Ok}(#{CustomizableOperation} { handle, operation })
                        }
                        """,
                        *middlewareScope,
                    )
                }
            }

            if (smithyRuntimeMode.generateOrchestrator) {
                val orchestratorScope = arrayOf(
                    *preludeScope,
                    "CustomizableOperation" to ClientRustModule.Client.customize.toType()
                        .resolve("orchestrator::CustomizableOperation"),
                    "HttpResponse" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                        .resolve("client::orchestrator::HttpResponse"),
                    "Operation" to operationSymbol,
                    "OperationError" to errorType,
                    "OperationOutput" to outputType,
                    "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
                    "SendResult" to ClientRustModule.Client.customize.toType()
                        .resolve("internal::SendResult"),
                    "SdkError" to RuntimeType.sdkError(runtimeConfig),
                )
                if (smithyRuntimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Sends the request and returns the response.
                        ///
                        /// If an error occurs, an `SdkError` will be returned with additional details that
                        /// can be matched against.
                        ///
                        /// By default, any retryable failures will be retried twice. Retry behavior
                        /// is configurable with the [RetryConfig](aws_smithy_types::retry::RetryConfig), which can be
                        /// set when configuring the client.
                        pub async fn send(self) -> #{Result}<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>> {
                            let input = self.inner.build().map_err(#{SdkError}::construction_failure)?;
                            let runtime_plugins = #{Operation}::operation_runtime_plugins(
                                self.handle.runtime_plugins.clone(),
                                &self.handle.conf,
                                self.config_override,
                            );
                            #{Operation}::orchestrate(&runtime_plugins, input).await
                        }

                        /// Consumes this builder, creating a customizable operation that can be modified before being
                        /// sent.
                        // TODO(enableNewSmithyRuntimeCleanup): Remove `async` and `Result` once we switch to orchestrator
                        pub async fn customize(
                            self,
                        ) -> #{Result}<
                            #{CustomizableOperation}<
                                #{OperationOutput},
                                #{OperationError},
                            >,
                            #{SdkError}<#{OperationError}>,
                        >
                        {
                            #{Ok}(#{CustomizableOperation} {
                                customizable_send: #{Box}::new(move |config_override| {
                                    #{Box}::pin(async {
                                        self.config_override(config_override)
                                            .send()
                                            .await
                                    })
                                }),
                                config_override: None,
                                interceptors: vec![],
                                runtime_plugins: vec![],
                            })
                        }
                        """,
                        *orchestratorScope,
                    )
                }

                rustTemplate(
                    """
                    pub(crate) fn config_override(
                        mut self,
                        config_override: impl Into<crate::config::Builder>,
                    ) -> Self {
                        self.set_config_override(Some(config_override.into()));
                        self
                    }

                    pub(crate) fn set_config_override(
                        &mut self,
                        config_override: Option<crate::config::Builder>,
                    ) -> &mut Self {
                        self.config_override = config_override;
                        self
                    }
                    """,
                )
            }

            PaginatorGenerator.paginatorType(codegenContext, generics, operation, retryClassifier)
                ?.also { paginatorType ->
                    rustTemplate(
                        """
                        /// Create a paginator for this request
                        ///
                        /// Paginators are used by calling [`send().await`](#{Paginator}::send) which returns a `Stream`.
                        pub fn into_paginator(self) -> #{Paginator}${generics.inst} {
                            #{Paginator}::new(self.handle, self.inner)
                        }
                        """,
                        "Paginator" to paginatorType,
                    )
                }
            writeCustomizations(
                customizations,
                FluentClientSection.FluentBuilderImpl(
                    operation,
                    symbolProvider.symbolForOperationError(operation),
                ),
            )
            input.members().forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                // All fields in the builder are optional
                val memberSymbol = symbolProvider.toSymbol(member)
                val outerType = memberSymbol.rustType()
                when (val coreType = outerType.stripOuter<RustType.Option>()) {
                    is RustType.Vec -> with(core) { renderVecHelper(member, memberName, coreType) }
                    is RustType.HashMap -> with(core) { renderMapHelper(member, memberName, coreType) }
                    else -> with(core) { renderInputHelper(member, memberName, coreType) }
                }
                // pure setter
                val setterName = member.setterName()
                val optionalInputType = outerType.asOptional()
                with(core) { renderInputHelper(member, setterName, optionalInputType) }

                val getterName = member.getterName()
                with(core) { renderGetterHelper(member, getterName, optionalInputType) }
            }
        }
    }
}

private fun baseClientRuntimePluginsFn(runtimeConfig: RuntimeConfig): RuntimeType =
    RuntimeType.forInlineFun("base_client_runtime_plugins", ClientRustModule.config) {
        rustTemplate(
            """
            pub(crate) fn base_client_runtime_plugins(
                mut config: crate::Config,
            ) -> #{RuntimePlugins} {
                let mut configured_plugins = #{Vec}::new();
                ::std::mem::swap(&mut config.runtime_plugins, &mut configured_plugins);
                let mut plugins = #{RuntimePlugins}::new()
                    .with_client_plugin(
                        #{StaticRuntimePlugin}::new()
                            .with_config(config.config.clone())
                            .with_runtime_components(config.runtime_components.clone())
                    )
                    .with_client_plugin(crate::config::ServiceRuntimePlugin::new(config))
                    .with_client_plugin(#{NoAuthRuntimePlugin}::new());
                for plugin in configured_plugins {
                    plugins = plugins.with_client_plugin(plugin);
                }
                plugins
            }
            """,
            *preludeScope,
            "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
            "NoAuthRuntimePlugin" to RuntimeType.smithyRuntime(runtimeConfig)
                .resolve("client::auth::no_auth::NoAuthRuntimePlugin"),
            "StaticRuntimePlugin" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                .resolve("client::runtime_plugin::StaticRuntimePlugin"),
        )
    }

/**
 * For a given `operation` shape, return a list of strings where each string describes the name and input type of one of
 * the operation's corresponding fluent builder methods as well as that method's documentation from the smithy model
 *
 * _NOTE: This function generates the docs that appear under **"The fluent builder is configurable:"**_
 */
private fun generateOperationShapeDocs(
    writer: RustWriter,
    symbolProvider: RustSymbolProvider,
    operation: OperationShape,
    model: Model,
): List<String> {
    val input = operation.inputShape(model)
    val fluentBuilderFullyQualifiedName = operation.fullyQualifiedFluentBuilder(symbolProvider)
    return input.members().map { memberShape ->
        val builderInputDoc = memberShape.asFluentBuilderInputDoc(symbolProvider)
        val builderInputLink = docLink("$fluentBuilderFullyQualifiedName::${symbolProvider.toMemberName(memberShape)}")
        val builderSetterDoc = memberShape.asFluentBuilderSetterDoc(symbolProvider)
        val builderSetterLink = docLink("$fluentBuilderFullyQualifiedName::${memberShape.setterName()}")

        val docTrait = memberShape.getMemberTrait(model, DocumentationTrait::class.java).orNull()
        val docs = when (docTrait?.value?.isNotBlank()) {
            true -> normalizeHtml(writer.escape(docTrait.value)).replace("\n", " ")
            else -> "(undocumented)"
        }

        "[`$builderInputDoc`]($builderInputLink) / [`$builderSetterDoc`]($builderSetterLink): $docs"
    }
}

/**
 * For a give `struct` shape, return a list of strings where each string describes the name and type of a struct field
 * as well as that field's documentation from the smithy model
 *
 *  * _NOTE: This function generates the list of types that appear under **"On success, responds with"**_
 */
private fun generateShapeMemberDocs(
    writer: RustWriter,
    symbolProvider: SymbolProvider,
    shape: StructureShape,
    model: Model,
): List<String> {
    val structName = symbolProvider.toSymbol(shape).rustType().qualifiedName()
    return shape.members().map { memberShape ->
        val name = symbolProvider.toMemberName(memberShape)
        val member = symbolProvider.toSymbol(memberShape).rustType().render(fullyQualified = false)
        val docTrait = memberShape.getMemberTrait(model, DocumentationTrait::class.java).orNull()
        val docs = when (docTrait?.value?.isNotBlank()) {
            true -> normalizeHtml(writer.escape(docTrait.value)).replace("\n", " ")
            else -> "(undocumented)"
        }

        "[`$name($member)`](${docLink("$structName::$name")}): $docs"
    }
}

internal fun OperationShape.fluentBuilderType(symbolProvider: RustSymbolProvider): RuntimeType =
    symbolProvider.moduleForBuilder(this).toType()
        .resolve(symbolProvider.toSymbol(this).name + "FluentBuilder")

/**
 * Generate a valid fully-qualified Type for a fluent builder e.g.
 * `OperationShape(AssumeRole)` -> `"crate::operations::assume_role::AssumeRoleFluentBuilder"`
 *
 *  * _NOTE: This function generates the links that appear under **"The fluent builder is configurable:"**_
 */
private fun OperationShape.fullyQualifiedFluentBuilder(
    symbolProvider: RustSymbolProvider,
): String = fluentBuilderType(symbolProvider).fullyQualifiedName()

/**
 * Generate a string that looks like a Rust function pointer for documenting a fluent builder method e.g.
 * `<MemberShape representing a struct method>` -> `"method_name(MethodInputType)"`
 *
 * _NOTE: This function generates the type names that appear under **"The fluent builder is configurable:"**_
 */
private fun MemberShape.asFluentBuilderInputDoc(symbolProvider: SymbolProvider): String {
    val memberName = symbolProvider.toMemberName(this)
    val outerType = symbolProvider.toSymbol(this).rustType()

    return "$memberName(${outerType.stripOuter<RustType.Option>().asArgumentType(fullyQualified = false)})"
}

/**
 * Generate a string that looks like a Rust function pointer for documenting a fluent builder setter method e.g.
 * `<MemberShape representing a struct method>` -> `"set_method_name(Option<MethodInputType>)"`
 *
 *  _NOTE: This function generates the setter type names that appear under **"The fluent builder is configurable:"**_
 */
private fun MemberShape.asFluentBuilderSetterDoc(symbolProvider: SymbolProvider): String {
    val memberName = this.setterName()
    val outerType = symbolProvider.toSymbol(this).rustType()

    return "$memberName(${outerType.asArgumentType(fullyQualified = false)})"
}
