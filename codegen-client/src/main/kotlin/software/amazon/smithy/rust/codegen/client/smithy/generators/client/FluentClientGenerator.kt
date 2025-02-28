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
import software.amazon.smithy.rust.codegen.client.smithy.generators.isPaginated
import software.amazon.smithy.rust.codegen.client.smithy.generators.waiters.WaitableGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.EscapeFor
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.asArgumentType
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docLink
import software.amazon.smithy.rust.codegen.core.rustlang.escape
import software.amazon.smithy.rust.codegen.core.rustlang.featureGatedBlock
import software.amazon.smithy.rust.codegen.core.rustlang.normalizeHtml
import software.amazon.smithy.rust.codegen.core.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
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
        fun clientOperationFnName(
            operationShape: OperationShape,
            symbolProvider: RustSymbolProvider,
        ): String = RustReservedWords.escapeIfNeeded(clientOperationFnDocsName(operationShape, symbolProvider))

        /** When using the function name in Rust docs, there's no need to escape Rust reserved words. **/
        fun clientOperationFnDocsName(
            operationShape: OperationShape,
            symbolProvider: RustSymbolProvider,
        ): String = symbolProvider.toSymbol(operationShape).name.toSnakeCase()

        fun clientOperationModuleName(
            operationShape: OperationShape,
            symbolProvider: RustSymbolProvider,
        ): String =
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

    fun render(crate: RustCrate) {
        renderFluentClient(crate)

        val customizableOperationGenerator = CustomizableOperationGenerator(codegenContext)
        operations.forEach { operation ->
            crate.withModule(symbolProvider.moduleForBuilder(operation)) {
                FluentBuilderGenerator(codegenContext, operation, customizations).render(this)
            }
        }

        customizableOperationGenerator.render(crate)

        WaitableGenerator(codegenContext, operations).render(crate)
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

                    fn validate_config(handle: &Handle) -> #{Result}<(), #{BoxError}> {
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
                "base_client_runtime_plugins" to baseClientRuntimePluginsFn(codegenContext, customizations),
                "BoxError" to RuntimeType.boxError(runtimeConfig),
                "client_docs" to
                    writable {
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
                    val maybePaginated =
                        if (operation.isPaginated(model)) {
                            "\n/// This operation supports pagination; See [`into_paginator()`]($fullPath::into_paginator)."
                        } else {
                            ""
                        }

                    val output = operation.outputShape(model)
                    val operationOk = symbolProvider.toSymbol(output)
                    val operationErr = symbolProvider.symbolForOperationError(operation)

                    val inputFieldsBody =
                        generateOperationShapeDocs(this, symbolProvider, operation, model)
                            .joinToString("\n") { "///   - $it" }

                    val inputFieldsHead =
                        if (inputFieldsBody.isNotEmpty()) {
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
}

private fun baseClientRuntimePluginsFn(
    codegenContext: ClientCodegenContext,
    customizations: List<FluentClientCustomization>,
): RuntimeType =
    codegenContext.runtimeConfig.let { rc ->
        RuntimeType.forInlineFun("base_client_runtime_plugins", ClientRustModule.config) {
            val api = RuntimeType.smithyRuntimeApiClient(rc)
            val rt = RuntimeType.smithyRuntime(rc)
            val behaviorVersionError =
                "Invalid client configuration: A behavior major version must be set when sending a " +
                    "request or constructing a client. You must set it during client construction or by enabling the " +
                    "`${BehaviorVersionLatest.name}` cargo feature."
            rustTemplate(
                """
                pub(crate) fn base_client_runtime_plugins(
                    mut config: crate::Config,
                ) -> #{RuntimePlugins} {
                    let mut configured_plugins = #{Vec}::new();
                    ::std::mem::swap(&mut config.runtime_plugins, &mut configured_plugins);
                    #{update_bmv}

                    let default_retry_partition = ${codegenContext.serviceShape.sdkId().dq()};
                    #{before_plugin_setup}

                    let scope = "aws.sdk.rust.services.${codegenContext.serviceShape.sdkId()}";

                    let mut plugins = #{RuntimePlugins}::new()
                        // defaults
                        .with_client_plugins(#{default_plugins}(
                            #{DefaultPluginParams}::new()
                                .with_retry_partition_name(default_retry_partition)
                                .with_behavior_version(config.behavior_version.expect(${behaviorVersionError.dq()}))
                                .with_time_source(config.runtime_components.time_source().unwrap_or_default())
                                .with_scope(scope)
                        ))
                        // user config
                        .with_client_plugin(
                            #{StaticRuntimePlugin}::new()
                                .with_config(config.config.clone())
                                .with_runtime_components(config.runtime_components.clone())
                        )
                        // codegen config
                        .with_client_plugin(crate::config::ServiceRuntimePlugin::new(config.clone()))
                        .with_client_plugin(#{NoAuthRuntimePlugin}::new());

                    #{additional_client_plugins:W}

                    for plugin in configured_plugins {
                        plugins = plugins.with_client_plugin(plugin);
                    }
                    plugins
                }
                """,
                *preludeScope,
                "additional_client_plugins" to
                    writable {
                        writeCustomizations(
                            customizations,
                            FluentClientSection.AdditionalBaseClientPlugins("plugins", "config"),
                        )
                    },
                "before_plugin_setup" to
                    writable {
                        writeCustomizations(
                            customizations,
                            FluentClientSection.BeforeBaseClientPluginSetup("config"),
                        )
                    },
                "DefaultPluginParams" to rt.resolve("client::defaults::DefaultPluginParams"),
                "default_plugins" to rt.resolve("client::defaults::default_plugins"),
                "NoAuthRuntimePlugin" to rt.resolve("client::auth::no_auth::NoAuthRuntimePlugin"),
                "RuntimePlugins" to RuntimeType.runtimePlugins(rc),
                "StaticRuntimePlugin" to api.resolve("client::runtime_plugin::StaticRuntimePlugin"),
                "update_bmv" to
                    featureGatedBlock(BehaviorVersionLatest) {
                        rustTemplate(
                            """
                            if config.behavior_version.is_none() {
                                config.behavior_version = Some(#{BehaviorVersion}::latest());
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
        val docs =
            when (docTrait?.value?.isNotBlank()) {
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
        val docs =
            when (docTrait?.value?.isNotBlank()) {
                true -> normalizeHtml(writer.escape(docTrait.value)).replace("\n", " ")
                else -> "(undocumented)"
            }

        "[`$name($member)`](${docLink("$structName::$name")}): $docs"
    }
}

fun OperationShape.fluentBuilderType(symbolProvider: RustSymbolProvider): RuntimeType =
    symbolProvider.moduleForBuilder(this).toType()
        .resolve(symbolProvider.toSymbol(this).name + "FluentBuilder")

/**
 * Generate a valid fully-qualified Type for a fluent builder e.g.
 * `OperationShape(AssumeRole)` -> `"crate::operations::assume_role::AssumeRoleFluentBuilder"`
 *
 *  * _NOTE: This function generates the links that appear under **"The fluent builder is configurable:"**_
 */
private fun OperationShape.fullyQualifiedFluentBuilder(symbolProvider: RustSymbolProvider): String =
    fluentBuilderType(symbolProvider).fullyQualifiedName()

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
    val renderedType =
        when (outerType) {
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
