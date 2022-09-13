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
import software.amazon.smithy.rust.codegen.client.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.client.rustlang.RustModule
import software.amazon.smithy.rust.codegen.client.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.client.rustlang.RustType
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.rustlang.Writable
import software.amazon.smithy.rust.codegen.client.rustlang.asArgumentType
import software.amazon.smithy.rust.codegen.client.rustlang.asOptional
import software.amazon.smithy.rust.codegen.client.rustlang.asType
import software.amazon.smithy.rust.codegen.client.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.client.rustlang.docLink
import software.amazon.smithy.rust.codegen.client.rustlang.docs
import software.amazon.smithy.rust.codegen.client.rustlang.documentShape
import software.amazon.smithy.rust.codegen.client.rustlang.escape
import software.amazon.smithy.rust.codegen.client.rustlang.normalizeHtml
import software.amazon.smithy.rust.codegen.client.rustlang.qualifiedName
import software.amazon.smithy.rust.codegen.client.rustlang.render
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.client.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.rustlang.rustTypeParameters
import software.amazon.smithy.rust.codegen.client.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.client.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.RustCrate
import software.amazon.smithy.rust.codegen.client.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.client.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.client.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.client.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.client.smithy.generators.GenericTypeArg
import software.amazon.smithy.rust.codegen.client.smithy.generators.GenericsGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.PaginatorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.client.smithy.generators.isPaginated
import software.amazon.smithy.rust.codegen.client.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.client.smithy.generators.smithyHttp
import software.amazon.smithy.rust.codegen.client.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase

class FluentClientGenerator(
    private val codegenContext: ClientCodegenContext,
    private val generics: FluentClientGenerics = FlexibleClientGenerics(
        connectorDefault = null,
        middlewareDefault = null,
        retryDefault = CargoDependency.SmithyClient(codegenContext.runtimeConfig).asType()
            .member("retry::Standard"),
        client = CargoDependency.SmithyClient(codegenContext.runtimeConfig).asType(),
    ),
    private val customizations: List<FluentClientCustomization> = emptyList(),
    private val retryPolicy: Writable = RustType.Unit.writable,
) {
    companion object {
        fun clientOperationFnName(operationShape: OperationShape, symbolProvider: RustSymbolProvider): String =
            RustReservedWords.escapeIfNeeded(symbolProvider.toSymbol(operationShape).name.toSnakeCase())

        val clientModule = RustModule.public(
            "client",
            "Client and fluent builders for calling the service.",
        )

        val customizableOperationModule = RustModule.public(
            "customizable_operation",
            "Wrap operations in a special type allowing for the modification of operations and the requests inside before sending them",
        )
    }

    private val serviceShape = codegenContext.serviceShape
    private val operations =
        TopDownIndex.of(codegenContext.model).getContainedOperations(serviceShape).sortedBy { it.id }
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model
    private val clientDep = CargoDependency.SmithyClient(codegenContext.runtimeConfig)
    private val runtimeConfig = codegenContext.runtimeConfig
    private val core = FluentClientCore(model)

    fun render(crate: RustCrate) {
        crate.withModule(clientModule) { writer ->
            renderFluentClient(writer)
        }

        crate.withModule(customizableOperationModule) { writer ->
            renderCustomizableOperationModule(runtimeConfig, generics, writer)

            if (codegenContext.settings.codegenConfig.includeFluentClient) {
                renderCustomizableOperationSend(runtimeConfig, generics, writer)
            }
        }
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
            "client" to clientDep.asType(),
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
            "client" to clientDep.asType(),
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
                val operationErr = operation.errorSymbol(model, symbolProvider, CodegenTarget.CLIENT).toSymbol()

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

                rustTemplate(
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

                rust(
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
        writer.withModule("fluent_builders") {
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
                val derives = baseDerives.derives.intersect(setOf(RuntimeType.Clone)) + RuntimeType.Debug
                rust(
                    """
                    /// Fluent builder constructing a request to `${operationSymbol.name}`.
                    ///
                    """,
                )

                documentShape(operation, model, autoSuppressMissingDocs = false)
                deprecatedShape(operation)
                baseDerives.copy(derives = derives).render(this)
                rustTemplate(
                    """
                    pub struct ${operationSymbol.name}#{generics:W} {
                        handle: std::sync::Arc<super::Handle${generics.inst}>,
                        inner: #{Inner}
                    }
                    """,
                    "Inner" to input.builderSymbol(symbolProvider),
                    "client" to clientDep.asType(),
                    "generics" to generics.decl,
                    "operation" to operationSymbol,
                )

                rustBlockTemplate(
                    "impl${generics.inst} ${operationSymbol.name}${generics.inst} #{bounds:W}",
                    "client" to clientDep.asType(),
                    "bounds" to generics.bounds,
                ) {
                    val outputType = symbolProvider.toSymbol(operation.outputShape(model))
                    val errorType = operation.errorSymbol(model, symbolProvider, CodegenTarget.CLIENT)

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
                            crate::customizable_operation::CustomizableOperation#{customizable_op_type_params:W},
                            #{SdkError}<#{OperationError}>
                        > #{send_bounds:W} {
                            let handle = self.handle.clone();
                            let operation = self.inner.build().map_err(|err|#{SdkError}::ConstructionFailure(err.into()))?
                                .make_operation(&handle.conf)
                                .await
                                .map_err(|err|#{SdkError}::ConstructionFailure(err.into()))?;
                            Ok(crate::customizable_operation::CustomizableOperation { handle, operation })
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
                            let op = self.inner.build().map_err(|err|#{SdkError}::ConstructionFailure(err.into()))?
                                .make_operation(&self.handle.conf)
                                .await
                                .map_err(|err|#{SdkError}::ConstructionFailure(err.into()))?;
                            self.handle.client.call(op).await
                        }
                        """,
                        "ClassifyResponse" to runtimeConfig.smithyHttp().member("retry::ClassifyResponse"),
                        "OperationError" to errorType,
                        "OperationOutput" to outputType,
                        "SdkError" to runtimeConfig.smithyHttp().member("result::SdkError"),
                        "SdkSuccess" to runtimeConfig.smithyHttp().member("result::SdkSuccess"),
                        "send_bounds" to generics.sendBounds(operationSymbol, outputType, errorType, retryPolicy),
                        "customizable_op_type_params" to rustTypeParameters(
                            symbolProvider.toSymbol(operation),
                            retryPolicy,
                            generics.toGenericsGenerator(),
                        ),
                    )
                    PaginatorGenerator.paginatorType(codegenContext, generics, operation, retryPolicy)?.also { paginatorType ->
                        rustTemplate(
                            """
                            /// Create a paginator for this request
                            ///
                            /// Paginators are used by calling [`send().await`](#{Paginator}::send) which returns a [`Stream`](tokio_stream::Stream).
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
                            operation.errorSymbol(model, symbolProvider, CodegenTarget.CLIENT),
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

private fun renderCustomizableOperationModule(
    runtimeConfig: RuntimeConfig,
    generics: FluentClientGenerics,
    writer: RustWriter,
) {
    val smithyHttp = CargoDependency.SmithyHttp(runtimeConfig).asType()

    val operationGenerics = GenericsGenerator(GenericTypeArg("O"), GenericTypeArg("Retry"))
    val handleGenerics = generics.toGenericsGenerator()
    val combinedGenerics = operationGenerics + handleGenerics

    val codegenScope = arrayOf(
        // SDK Types
        "http_result" to smithyHttp.member("result"),
        "http_body" to smithyHttp.member("body"),
        "http_operation" to smithyHttp.member("operation"),
        "HttpRequest" to CargoDependency.Http.asType().member("Request"),
        "handle_generics_decl" to handleGenerics.declaration(),
        "handle_generics_bounds" to handleGenerics.bounds(),
        "operation_generics_decl" to operationGenerics.declaration(),
        "combined_generics_decl" to combinedGenerics.declaration(),
    )

    writer.rustTemplate(
        """
        use crate::client::Handle;

        use #{http_body}::SdkBody;
        use #{http_operation}::Operation;
        use #{http_result}::SdkError;

        use std::convert::Infallible;
        use std::sync::Arc;

        /// A wrapper type for [`Operation`](aws_smithy_http::operation::Operation)s that allows for
        /// customization of the operation before it is sent. A `CustomizableOperation` may be sent
        /// by calling its [`.send()`][crate::customizable_operation::CustomizableOperation::send] method.
        ##[derive(Debug)]
        pub struct CustomizableOperation#{combined_generics_decl:W} {
            pub(crate) handle: Arc<Handle#{handle_generics_decl:W}>,
            pub(crate) operation: Operation#{operation_generics_decl:W},
        }

        impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
        where
            #{handle_generics_bounds:W}
        {
            /// Allows for customizing the operation's request
            pub fn map_request<E>(
                mut self,
                f: impl FnOnce(#{HttpRequest}<SdkBody>) -> Result<#{HttpRequest}<SdkBody>, E>,
            ) -> Result<Self, E> {
                let (request, response) = self.operation.into_request_response();
                let request = request.augment(|req, _props| f(req))?;
                self.operation = Operation::from_parts(request, response);
                Ok(self)
            }

            /// Convenience for `map_request` where infallible direct mutation of request is acceptable
            pub fn mutate_request<E>(self, f: impl FnOnce(&mut #{HttpRequest}<SdkBody>)) -> Self {
                self.map_request(|mut req| {
                    f(&mut req);
                    Result::<_, Infallible>::Ok(req)
                })
                .expect("infallible")
            }

            /// Allows for customizing the entire operation
            pub fn map_operation<E>(
                mut self,
                f: impl FnOnce(Operation#{operation_generics_decl:W}) -> Result<Operation#{operation_generics_decl:W}, E>,
            ) -> Result<Self, E> {
                self.operation = f(self.operation)?;
                Ok(self)
            }

            /// Direct access to read the HTTP request
            pub fn request(&self) -> &#{HttpRequest}<SdkBody> {
                self.operation.request()
            }

            /// Direct access to mutate the HTTP request
            pub fn request_mut(&mut self) -> &mut #{HttpRequest}<SdkBody> {
                self.operation.request_mut()
            }
        }
        """,
        *codegenScope,
    )
}

private fun renderCustomizableOperationSend(
    runtimeConfig: RuntimeConfig,
    generics: FluentClientGenerics,
    writer: RustWriter,
) {
    val smithyHttp = CargoDependency.SmithyHttp(runtimeConfig).asType()
    val smithyClient = CargoDependency.SmithyClient(runtimeConfig).asType()

    val operationGenerics = GenericsGenerator(GenericTypeArg("O"), GenericTypeArg("Retry"))
    val handleGenerics = generics.toGenericsGenerator()
    val combinedGenerics = operationGenerics + handleGenerics

    val codegenScope = arrayOf(
        "combined_generics_decl" to combinedGenerics.declaration(),
        "handle_generics_bounds" to handleGenerics.bounds(),
        "ParseHttpResponse" to smithyHttp.member("response::ParseHttpResponse"),
        "NewRequestPolicy" to smithyClient.member("retry::NewRequestPolicy"),
        "SmithyRetryPolicy" to smithyClient.member("bounds::SmithyRetryPolicy"),
    )

    writer.rustTemplate(
        """
        impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
        where
            #{handle_generics_bounds:W}
        {
            /// Sends this operation's request
            pub async fn send<T, E>(self) -> Result<T, SdkError<E>>
            where
                E: std::error::Error,
                O: #{ParseHttpResponse}<Output = Result<T, E>> + Send + Sync + Clone + 'static,
                Retry: Send + Sync + Clone,
                <R as #{NewRequestPolicy}>::Policy: #{SmithyRetryPolicy}<O, T, E, Retry> + Clone,
            {
                self.handle.client.call(self.operation).await
            }
        }
        """,
        *codegenScope,
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
