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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class FluentClientGenerator(
    private val codegenContext: ClientCodegenContext,
    private val reexportSmithyClientBuilder: Boolean = true,
    private val generics: FluentClientGenerics = FlexibleClientGenerics(
        connectorDefault = null,
        middlewareDefault = null,
        retryDefault = RuntimeType.smithyClient(codegenContext.runtimeConfig).resolve("retry::Standard"),
        client = RuntimeType.smithyClient(codegenContext.runtimeConfig),
    ),
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
    private val enableNewSmithyRuntime = codegenContext.settings.codegenConfig.enableNewSmithyRuntime

    fun render(crate: RustCrate) {
        renderFluentClient(crate)

        operations.forEach { operation ->
            crate.withModule(symbolProvider.moduleForBuilder(operation)) {
                renderFluentBuilder(operation)
            }
        }

        CustomizableOperationGenerator(codegenContext, generics).render(crate)
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
            rustTemplate(
                """
                ##[derive(Debug)]
                pub(crate) struct Handle#{generics_decl:W} {
                    pub(crate) client: #{client}::Client#{smithy_inst:W},
                    pub(crate) conf: crate::Config,
                }

                #{client_docs:W}
                ##[derive(std::fmt::Debug)]
                pub struct Client#{generics_decl:W} {
                    handle: std::sync::Arc<Handle${generics.inst}>
                }

                impl${generics.inst} std::clone::Clone for Client${generics.inst} {
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
                            handle: std::sync::Arc::new(Handle {
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
                "generics_decl" to generics.decl,
                "smithy_inst" to generics.smithyInst,
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
            )
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
        val operationSymbol = symbolProvider.toSymbol(operation)
        val input = operation.inputShape(model)
        val baseDerives = symbolProvider.toSymbol(input).expectRustMetadata().derives
        // Filter out any derive that isn't Clone. Then add a Debug derive
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
                handle: std::sync::Arc<crate::client::Handle${generics.inst}>,
                inner: #{Inner},
                """,
                "Inner" to symbolProvider.symbolForBuilder(input),
                "generics" to generics.decl,
            )
            if (enableNewSmithyRuntime) {
                rust("config_override: std::option::Option<crate::config::Builder>,")
            }
        }

        rustBlockTemplate(
            "impl${generics.inst} $builderName${generics.inst} #{bounds:W}",
            "client" to RuntimeType.smithyClient(runtimeConfig),
            "bounds" to generics.bounds,
        ) {
            val outputType = symbolProvider.toSymbol(operation.outputShape(model))
            val errorType = symbolProvider.symbolForOperationError(operation)
            val inputBuilderType = symbolProvider.symbolForBuilder(input)
            val fnName = clientOperationFnName(operation, symbolProvider)

            rust("/// Creates a new `${operationSymbol.name}`.")
            withBlockTemplate(
                "pub(crate) fn new(handle: std::sync::Arc<crate::client::Handle${generics.inst}>) -> Self {",
                "}",
                "generics" to generics.decl,
            ) {
                withBlockTemplate(
                    "Self {",
                    "}",
                ) {
                    rust("handle, inner: Default::default(),")
                    if (enableNewSmithyRuntime) {
                        rust("config_override: None,")
                    }
                }
            }
            rustTemplate(
                """
                /// Consume this builder, creating a customizable operation that can be modified before being
                /// sent. The operation's inner [http::Request] can be modified as well.
                pub async fn customize(self) -> std::result::Result<
                    #{CustomizableOperation}#{customizable_op_type_params:W},
                    #{SdkError}<#{OperationError}>
                > #{send_bounds:W} {
                    let handle = self.handle.clone();
                    let operation = self.inner.build().map_err(#{SdkError}::construction_failure)?
                        .make_operation(&handle.conf)
                        .await
                        .map_err(#{SdkError}::construction_failure)?;
                    Ok(#{CustomizableOperation} { handle, operation })
                }

                /// Sends the request and returns the response.
                ///
                /// If an error occurs, an `SdkError` will be returned with additional details that
                /// can be matched against.
                ///
                /// By default, any retryable failures will be retried twice. Retry behavior
                /// is configurable with the [RetryConfig](aws_smithy_types::retry::RetryConfig), which can be
                /// set when configuring the client.
                pub async fn send(self) -> std::result::Result<#{OperationOutput}, #{SdkError}<#{OperationError}>>
                #{send_bounds:W} {
                    let op = self.inner.build().map_err(#{SdkError}::construction_failure)?
                        .make_operation(&self.handle.conf)
                        .await
                        .map_err(#{SdkError}::construction_failure)?;
                    self.handle.client.call(op).await
                }

                """,
                "CustomizableOperation" to ClientRustModule.Client.customize.toType()
                    .resolve("CustomizableOperation"),
                "ClassifyRetry" to RuntimeType.classifyRetry(runtimeConfig),
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

            // this fixes this error
            //  error[E0592]: duplicate definitions with name `set_fields`
            //     --> sdk/connectcases/src/operation/update_case/builders.rs:115:5
            //      |
            //  78  | /     pub fn set_fields(
            //  79  | |         mut self,
            //  80  | |         data: crate::operation::update_case::builders::UpdateCaseInputBuilder,
            //  81  | |     ) -> Self {
            //      | |_____________- other definition for `set_fields`
            //  ...
            //  115 | /     pub fn set_fields(
            //  116 | |         mut self,
            //  117 | |         input: std::option::Option<std::vec::Vec<crate::types::FieldValue>>,
            //  118 | |     ) -> Self {
            //      | |_____________^ duplicate definitions for `set_fields`
            if (inputBuilderType.toString().endsWith("Builder")) {
                rustTemplate(
                    """
                    ##[#{AwsSdkUnstableAttribute}]
                    /// This function replaces the parameter with new one.
                    /// It is useful when you want to replace the existing data with de-serialized data.
                    /// ```compile_fail
                    /// let result_future = async {
                    ///     let deserialized_parameters: $inputBuilderType  = serde_json::from_str(&json_string).unwrap();
                    ///     client.$fnName().set_fields(&deserialized_parameters).send().await
                    /// };
                    /// ```
                    pub fn set_fields(mut self, data: $inputBuilderType) -> Self {
                        self.inner = data;
                        self
                    }
                    """,
                    "AwsSdkUnstableAttribute" to Attribute.AwsSdkUnstableAttribute.inner,
                )
            }

            PaginatorGenerator.paginatorType(codegenContext, generics, operation, retryClassifier)?.also { paginatorType ->
            if (enableNewSmithyRuntime) {
                rustTemplate(
                    """
                    // TODO(enableNewSmithyRuntime): Replace `send` with `send_v2`
                    /// Sends the request and returns the response.
                    ///
                    /// If an error occurs, an `SdkError` will be returned with additional details that
                    /// can be matched against.
                    ///
                    /// By default, any retryable failures will be retried twice. Retry behavior
                    /// is configurable with the [RetryConfig](aws_smithy_types::retry::RetryConfig), which can be
                    /// set when configuring the client.
                    pub async fn send_v2(self) -> std::result::Result<#{OperationOutput}, #{SdkError}<#{OperationError}, #{HttpResponse}>> {
                        let mut runtime_plugins = #{RuntimePlugins}::new()
                            .with_client_plugin(crate::config::ServiceRuntimePlugin::new(self.handle.clone()));
                        if let Some(config_override) = self.config_override {
                            runtime_plugins = runtime_plugins.with_operation_plugin(config_override);
                        }
                        runtime_plugins = runtime_plugins.with_operation_plugin(#{Operation}::new());
                        let input = self.inner.build().map_err(#{SdkError}::construction_failure)?;
                        let input = #{TypedBox}::new(input).erase();
                        let output = #{invoke}(input, &runtime_plugins)
                            .await
                            .map_err(|err| {
                                err.map_service_error(|err| {
                                    #{TypedBox}::<#{OperationError}>::assume_from(err)
                                        .expect("correct error type")
                                        .unwrap()
                                })
                            })?;
                        Ok(#{TypedBox}::<#{OperationOutput}>::assume_from(output).expect("correct output type").unwrap())
                    }
                    """,
                    "HttpResponse" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                        .resolve("client::orchestrator::HttpResponse"),
                    "OperationError" to errorType,
                    "Operation" to symbolProvider.toSymbol(operation),
                    "OperationOutput" to outputType,
                    "RuntimePlugins" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                        .resolve("client::runtime_plugin::RuntimePlugins"),
                    "SdkError" to RuntimeType.sdkError(runtimeConfig),
                    "TypedBox" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("type_erasure::TypedBox"),
                    "invoke" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::orchestrator::invoke"),
                )

                rustTemplate(
                    """
                    /// Sets the `config_override` for the builder.
                    ///
                    /// `config_override` is applied to the operation configuration level.
                    /// The fields in the builder that are `Some` override those applied to the service
                    /// configuration level. For instance,
                    ///
                    /// Config A     overridden by    Config B          ==        Config C
                    /// field_1: None,                field_1: Some(v2),          field_1: Some(v2),
                    /// field_2: Some(v1),            field_2: Some(v2),          field_2: Some(v2),
                    /// field_3: Some(v1),            field_3: None,              field_3: Some(v1),
                    pub fn config_override(
                        mut self,
                        config_override: impl Into<crate::config::Builder>,
                    ) -> Self {
                        self.set_config_override(Some(config_override.into()));
                        self
                    }

                    /// Sets the `config_override` for the builder.
                    ///
                    /// `config_override` is applied to the operation configuration level.
                    /// The fields in the builder that are `Some` override those applied to the service
                    /// configuration level. For instance,
                    ///
                    /// Config A     overridden by    Config B          ==        Config C
                    /// field_1: None,                field_1: Some(v2),          field_1: Some(v2),
                    /// field_2: Some(v1),            field_2: Some(v2),          field_2: Some(v2),
                    /// field_3: Some(v1),            field_3: None,              field_3: Some(v1),
                    pub fn set_config_override(
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
            }
        }
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
