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
import software.amazon.smithy.rust.codegen.client.smithy.generators.PaginatorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.isPaginated
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute.Companion.derive
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
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
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class FluentClientGenerator(
    private val codegenContext: ClientCodegenContext,
    private val generics: FluentClientGenerics = FlexibleClientGenerics(
        connectorDefault = null,
        middlewareDefault = null,
        retryDefault = RuntimeType.smithyClient(codegenContext.runtimeConfig).resolve("retry::Standard"),
        client = RuntimeType.smithyClient(codegenContext.runtimeConfig),
    ),
    private val customizations: List<FluentClientCustomization> = emptyList(),
    private val retryClassifier: RuntimeType = RuntimeType.smithyHttp(codegenContext.runtimeConfig).resolve("retry::DefaultResponseRetryClassifier"),
) {
    companion object {
        fun clientOperationFnName(operationShape: OperationShape, symbolProvider: RustSymbolProvider): String =
            RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operationShape).name.toSnakeCase())

        val clientModule = RustModule.public(
            "client",
            "Client and fluent builders for calling the service.",
        )
    }

    private val serviceShape = codegenContext.serviceShape
    private val operations =
        TopDownIndex.of(codegenContext.model).getContainedOperations(serviceShape).sortedBy { it.id }
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val core = FluentClientCore(model)

    fun render(crate: RustCrate) {
        crate.withModule(clientModule) {
            renderFluentClient(this)
        }

        CustomizableOperationGenerator(
            runtimeConfig,
            generics,
            codegenContext.settings.codegenConfig.includeFluentClient,
        ).render(crate)
    }

    private fun renderFluentClient(writer: RustWriter) {
        writer.rustTemplate(
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

            ##[doc(inline)]
            pub use #{client}::Builder;

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
        writer.rustBlockTemplate(
            "impl${generics.inst} Client${generics.inst} #{bounds:W}",
            "client" to RuntimeType.smithyClient(runtimeConfig),
            "bounds" to generics.bounds,
        ) {
            operations.forEach { operation ->
                val name = symbolProvider.toSymbol(operation).name
                val fullPath = operation.fullyQualifiedFluentBuilder(symbolProvider)
                val maybePaginated = if (operation.isPaginated(model)) {
                    "\n/// This operation supports pagination; See [`into_paginator()`]($fullPath::into_paginator)."
                } else ""

                val output = operation.outputShape(model)
                val operationOk = symbolProvider.toSymbol(output)
                val operationErr = operation.errorSymbol(symbolProvider).toSymbol()

                val inputFieldsBody =
                    generateOperationShapeDocs(writer, symbolProvider, operation, model).joinToString("\n") {
                        "///   - $it"
                    }

                val inputFieldsHead = if (inputFieldsBody.isNotEmpty()) {
                    "The fluent builder is configurable:"
                } else {
                    "The fluent builder takes no input, just [`send`]($fullPath::send) it."
                }

                val outputFieldsBody =
                    generateShapeMemberDocs(writer, symbolProvider, output, model).joinToString("\n") {
                        "///   - $it"
                    }

                var outputFieldsHead = "On success, responds with [`${operationOk.name}`]($operationOk)"
                if (outputFieldsBody.isNotEmpty()) {
                    outputFieldsHead += " with field(s):"
                }

                writer.rustTemplate(
                    """
                    /// Constructs a fluent builder for the [`$name`]($fullPath) operation.$maybePaginated
                    ///
                    /// - $inputFieldsHead
                    $inputFieldsBody
                    /// - $outputFieldsHead
                    $outputFieldsBody
                    /// - On failure, responds with [`SdkError<${operationErr.name}>`]($operationErr)
                    """,
                )

                writer.rust(
                    """
                    pub fn ${
                    clientOperationFnName(
                        operation,
                        symbolProvider,
                    )
                    }(&self) -> fluent_builders::$name${generics.inst} {
                        fluent_builders::$name::new(self.handle.clone())
                    }
                    """,
                )
            }
        }
        writer.withInlineModule(RustModule.new("fluent_builders", visibility = Visibility.PUBLIC, inline = true)) {
            docs(
                """
                Utilities to ergonomically construct a request to the service.

                Fluent builders are created through the [`Client`](crate::client::Client) by calling
                one if its operation methods. After parameters are set using the builder methods,
                the `send` method can be called to initiate the request.
                """.trim(),
                newlinePrefix = "//! ",
            )
            operations.forEach { operation ->
                val operationSymbol = symbolProvider.toSymbol(operation)
                val input = operation.inputShape(model)
                val baseDerives = symbolProvider.toSymbol(input).expectRustMetadata().derives
                // Filter out any derive that isn't Clone. Then add a Debug derive
                val derives = baseDerives.filter { it == RuntimeType.Clone } + RuntimeType.Debug
                rust(
                    """
                    /// Fluent builder constructing a request to `${operationSymbol.name}`.
                    ///
                    """,
                )

                documentShape(operation, model, autoSuppressMissingDocs = false)
                deprecatedShape(operation)
                Attribute(derive(derives.toSet())).render(this)
                rustTemplate(
                    """
                    pub struct ${operationSymbol.name}#{generics:W} {
                        handle: std::sync::Arc<super::Handle${generics.inst}>,
                        inner: #{Inner}
                    }
                    """,
                    "Inner" to input.builderSymbol(symbolProvider),
                    "client" to RuntimeType.smithyClient(runtimeConfig),
                    "generics" to generics.decl,
                    "operation" to operationSymbol,
                )

                rustBlockTemplate(
                    "impl${generics.inst} ${operationSymbol.name}${generics.inst} #{bounds:W}",
                    "client" to RuntimeType.smithyClient(runtimeConfig),
                    "bounds" to generics.bounds,
                ) {
                    val outputType = symbolProvider.toSymbol(operation.outputShape(model))
                    val errorType = operation.errorSymbol(symbolProvider)
                    val operationFnName = clientOperationFnName(
                        operation,
                        symbolProvider,
                    )
                    // Have to use fully-qualified result here or else it could conflict with an op named Result
                    rustTemplate(
                        """
                        /// Creates a new `${operationSymbol.name}`.
                        pub(crate) fn new(handle: std::sync::Arc<super::Handle${generics.inst}>) -> Self {
                            Self { handle, inner: Default::default() }
                        }

                        /// Consume this builder, creating a customizable operation that can be modified before being
                        /// sent. The operation's inner [http::Request] can be modified as well.
                        pub async fn customize(self) -> std::result::Result<
                            crate::operation::customize::CustomizableOperation#{customizable_op_type_params:W},
                            #{SdkError}<#{OperationError}>
                        > #{send_bounds:W} {
                            let handle = self.handle.clone();
                            let operation = self.inner.build().map_err(#{SdkError}::construction_failure)?
                                .make_operation(&handle.conf)
                                .await
                                .map_err(#{SdkError}::construction_failure)?;
                            Ok(crate::operation::customize::CustomizableOperation { handle, operation })
                        }

                        ##[#{Unstable}]
                        /// This function replaces the parameter with new one.
                        /// It is useful when you want to replace the existing data with de-serialized data.
                        /// ```rust
                        /// let deserialized_parameters: #{InputBuilderType}  = serde_json::from_str(parameters_written_in_json).unwrap();
                        /// let outcome: #{OperationOutput} = client.$operationFnName().set_fields(&deserialized_parameters).send().await;
                        /// ```
                        pub fn set_fields(mut self, data: #{InputBuilderType}) -> Self {
                            self.inner = data;
                            self
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
                        "Unstable" to Attribute.AwsSdkUnstableAttribute.inner,
                        "InputBuilderType" to input.builderSymbol(symbolProvider),
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
                    PaginatorGenerator.paginatorType(codegenContext, generics, operation, retryClassifier)?.also { paginatorType ->
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
                            operation.errorSymbol(symbolProvider),
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
    symbolProvider: SymbolProvider,
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

/**
 * Generate a valid fully-qualified Type for a fluent builder e.g.
 * `OperationShape(AssumeRole)` -> `"crate::client::fluent_builders::AssumeRole"`
 *
 *  * _NOTE: This function generates the links that appear under **"The fluent builder is configurable:"**_
 */
private fun OperationShape.fullyQualifiedFluentBuilder(symbolProvider: SymbolProvider): String {
    val operationName = symbolProvider.toSymbol(this).name

    return "crate::client::fluent_builders::$operationName"
}

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
