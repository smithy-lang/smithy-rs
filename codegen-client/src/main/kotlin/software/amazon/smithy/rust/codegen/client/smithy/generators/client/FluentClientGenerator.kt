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
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.EscapeFor
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
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
import software.amazon.smithy.rust.codegen.core.rustlang.featureGatedBlock
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.normalizeHtml
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.getterName
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.sdkId
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

private val BehaviorVersionLatest = Feature("behavior-version-latest", false, listOf())
class FluentClientGenerator(
    private val codegenContext: ClientCodegenContext,
    private val customizations: List<FluentClientCustomization> = emptyList(),
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

    fun render(crate: RustCrate, customizableOperationCustomizations: List<CustomizableOperationCustomization> = emptyList()) {
        renderFluentClient(crate)

        val customizableOperationGenerator = CustomizableOperationGenerator(codegenContext, customizableOperationCustomizations)
        operations.forEach { operation ->
            crate.withModule(symbolProvider.moduleForBuilder(operation)) {
                renderFluentBuilder(operation)
            }
        }

        customizableOperationGenerator.render(crate)
    }

    private fun renderFluentClient(crate: RustCrate) {
        crate.mergeFeature(BehaviorVersionLatest)
        crate.withModule(ClientRustModule.client) {
            rustTemplate(
                """
                ##[derive(Debug)]
                pub(crate) struct Handle {
                    pub(crate) conf: crate::Config,
                    ##[allow(dead_code)] // unused when a service does not provide any operations
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
                    /// This method will panic in the following cases:
                    ///
                    /// - Retries or timeouts are enabled without a `sleep_impl` configured.
                    /// - Identity caching is enabled without a `sleep_impl` and `time_source` configured.
                    /// - No `behavior_version` is provided.
                    ///
                    /// The panic message for each of these will have instructions on how to resolve them.
                    ##[track_caller]
                    pub fn from_conf(conf: crate::Config) -> Self {
                        let handle = Handle {
                            conf: conf.clone(),
                            runtime_plugins: #{base_client_runtime_plugins}(conf),
                        };
                        if let Err(err) = Self::validate_config(&handle) {
                            panic!("Invalid client configuration: {err}");
                        }
                        Self {
                            handle: #{Arc}::new(handle)
                        }
                    }

                    /// Returns the client's configuration.
                    pub fn config(&self) -> &crate::Config {
                        &self.handle.conf
                    }

                    fn validate_config(handle: &Handle) -> Result<(), #{BoxError}> {
                        let mut cfg = #{ConfigBag}::base();
                        handle.runtime_plugins
                            .apply_client_configuration(&mut cfg)?
                            .validate_base_client_config(&cfg)?;
                        Ok(())
                    }
                }
                """,
                *preludeScope,
                "Arc" to RuntimeType.Arc,
                "base_client_runtime_plugins" to baseClientRuntimePluginsFn(codegenContext),
                "BoxError" to RuntimeType.boxError(runtimeConfig),
                "client_docs" to writable {
                    customizations.forEach {
                        it.section(
                            FluentClientSection.FluentClientDocs(
                                serviceShape,
                            ),
                        )(this)
                    }
                },
                "ConfigBag" to RuntimeType.configBag(runtimeConfig),
                "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
                "tracing" to CargoDependency.Tracing.toType(),
            )
        }

        operations.forEach { operation ->
            val name = symbolProvider.toSymbol(operation).name
            val fnName = clientOperationFnName(operation, symbolProvider)
            val moduleName = clientOperationModuleName(operation, symbolProvider)

            val privateModule = RustModule.private(moduleName, parent = ClientRustModule.client)
            crate.withModule(privateModule) {
                rustBlock("impl super::Client") {
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
                        pub fn $fnName(&self) -> #{FluentBuilder} {
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
                pub async fn send_with(self, client: &crate::Client) -> #{Result}<
                    #{OperationOutput},
                    #{SdkError}<
                        #{OperationError},
                        #{RawResponseType}
                    >
                > {
                    let mut fluent_builder = client.$fnName();
                    fluent_builder.inner = self;
                    fluent_builder.send().await
                }
                """,
                *preludeScope,
                "RawResponseType" to
                    RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::orchestrator::HttpResponse"),
                "Operation" to operationSymbol,
                "OperationError" to errorType,
                "OperationOutput" to outputType,
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
            )
        }

        val derives = baseDerives.filter { it == RuntimeType.Clone } + RuntimeType.Debug
        docs("Fluent builder constructing a request to `${operationSymbol.name}`.\n")

        val builderName = operation.fluentBuilderType(symbolProvider).name
        documentShape(operation, model, autoSuppressMissingDocs = false)
        deprecatedShape(operation)
        Attribute(derive(derives.toSet())).render(this)
        withBlockTemplate(
            "pub struct $builderName {",
            "}",
        ) {
            rustTemplate(
                """
                handle: #{Arc}<crate::client::Handle>,
                inner: #{Inner},
                """,
                "Inner" to symbolProvider.symbolForBuilder(input),
                "Arc" to RuntimeType.Arc,
            )
            rustTemplate("config_override: #{Option}<crate::config::Builder>,", *preludeScope)
        }

        rustTemplate(
            """
            impl
                crate::client::customize::internal::CustomizableSend<
                    #{OperationOutput},
                    #{OperationError},
                > for $builderName
            {
                fn send(
                    self,
                    config_override: crate::config::Builder,
                ) -> crate::client::customize::internal::BoxFuture<
                    crate::client::customize::internal::SendResult<
                        #{OperationOutput},
                        #{OperationError},
                    >,
                > {
                    #{Box}::pin(async move { self.config_override(config_override).send().await })
                }
            }
            """,
            *preludeScope,
            "OperationError" to errorType,
            "OperationOutput" to outputType,
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
        )

        rustBlock("impl $builderName") {
            rust("/// Creates a new `${operationSymbol.name}`.")
            withBlockTemplate(
                "pub(crate) fn new(handle: #{Arc}<crate::client::Handle>) -> Self {",
                "}",
                "Arc" to RuntimeType.Arc,
            ) {
                withBlockTemplate(
                    "Self {",
                    "}",
                ) {
                    rustTemplate("handle, inner: #{Default}::default(),", *preludeScope)
                    rustTemplate("config_override: #{None},", *preludeScope)
                }
            }

            rust("/// Access the ${operationSymbol.name} as a reference.\n")
            withBlockTemplate(
                "pub fn as_input(&self) -> &#{Inner} {", "}",
                "Inner" to symbolProvider.symbolForBuilder(input),
            ) {
                write("&self.inner")
            }

            val orchestratorScope = arrayOf(
                *preludeScope,
                "CustomizableOperation" to ClientRustModule.Client.customize.toType()
                    .resolve("CustomizableOperation"),
                "HttpResponse" to RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::orchestrator::HttpResponse"),
                "Operation" to operationSymbol,
                "OperationError" to errorType,
                "OperationOutput" to outputType,
                "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
                "SendResult" to ClientRustModule.Client.customize.toType()
                    .resolve("internal::SendResult"),
                "SdkError" to RuntimeType.sdkError(runtimeConfig),
            )
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

                /// Consumes this builder, creating a customizable operation that can be modified before being sent.
                pub fn customize(
                    self,
                ) -> #{CustomizableOperation}<#{OperationOutput}, #{OperationError}, Self> {
                    #{CustomizableOperation}::new(self)
                }
                """,
                *orchestratorScope,
            )

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

            PaginatorGenerator.paginatorType(codegenContext, operation)
                ?.also { paginatorType ->
                    rustTemplate(
                        """
                        /// Create a paginator for this request
                        ///
                        /// Paginators are used by calling [`send().await`](#{Paginator}::send) which returns a [`PaginationStream`](aws_smithy_async::future::pagination_stream::PaginationStream).
                        pub fn into_paginator(self) -> #{Paginator} {
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

private fun baseClientRuntimePluginsFn(codegenContext: ClientCodegenContext): RuntimeType = codegenContext.runtimeConfig.let { rc ->
    RuntimeType.forInlineFun("base_client_runtime_plugins", ClientRustModule.config) {
        val api = RuntimeType.smithyRuntimeApiClient(rc)
        val rt = RuntimeType.smithyRuntime(rc)
        val behaviorVersionError = "Invalid client configuration: A behavior major version must be set when sending a " +
            "request or constructing a client. You must set it during client construction or by enabling the " +
            "`${BehaviorVersionLatest.name}` cargo feature."
        rustTemplate(
            """
            pub(crate) fn base_client_runtime_plugins(
                mut config: crate::Config,
            ) -> #{RuntimePlugins} {
                let mut configured_plugins = #{Vec}::new();
                ::std::mem::swap(&mut config.runtime_plugins, &mut configured_plugins);
                ##[allow(unused_mut)]
                let mut behavior_version = config.behavior_version.clone();
                #{update_bmv}

                let mut plugins = #{RuntimePlugins}::new()
                    // defaults
                    .with_client_plugins(#{default_plugins}(
                        #{DefaultPluginParams}::new()
                            .with_retry_partition_name(${codegenContext.serviceShape.sdkId().dq()})
                            .with_behavior_version(behavior_version.expect(${behaviorVersionError.dq()}))
                    ))
                    // user config
                    .with_client_plugin(
                        #{StaticRuntimePlugin}::new()
                            .with_config(config.config.clone())
                            .with_runtime_components(config.runtime_components.clone())
                    )
                    // codegen config
                    .with_client_plugin(crate::config::ServiceRuntimePlugin::new(config))
                    .with_client_plugin(#{NoAuthRuntimePlugin}::new());

                for plugin in configured_plugins {
                    plugins = plugins.with_client_plugin(plugin);
                }
                plugins
            }
            """,
            *preludeScope,
            "DefaultPluginParams" to rt.resolve("client::defaults::DefaultPluginParams"),
            "default_plugins" to rt.resolve("client::defaults::default_plugins"),
            "NoAuthRuntimePlugin" to rt.resolve("client::auth::no_auth::NoAuthRuntimePlugin"),
            "RuntimePlugins" to RuntimeType.runtimePlugins(rc),
            "StaticRuntimePlugin" to api.resolve("client::runtime_plugin::StaticRuntimePlugin"),
            "update_bmv" to featureGatedBlock(BehaviorVersionLatest) {
                rustTemplate(
                    """
                    if behavior_version.is_none() {
                        behavior_version = Some(#{BehaviorVersion}::latest());
                    }

                    """,
                    "BehaviorVersion" to api.resolve("client::behavior_version::BehaviorVersion"),
                )
            },
        )
    }
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

        "[`$builderInputDoc`]($builderInputLink) / [`$builderSetterDoc`]($builderSetterLink):<br>required: **${memberShape.isRequired}**<br>$docs<br>"
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
internal fun MemberShape.asFluentBuilderInputDoc(symbolProvider: SymbolProvider): String {
    val memberName = symbolProvider.toMemberName(this)
    val outerType = symbolProvider.toSymbol(this).rustType().stripOuter<RustType.Option>()
    // We generate Vec/HashMap helpers
    val renderedType = when (outerType) {
        is RustType.Vec -> listOf(outerType.member)
        is RustType.HashMap -> listOf(outerType.key, outerType.member)
        else -> listOf(outerType)
    }
    val args = renderedType.joinToString { it.asArgumentType(fullyQualified = false) }

    return "$memberName($args)"
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
