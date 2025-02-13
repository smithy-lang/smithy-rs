/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.PaginatorGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asArgument
import software.amazon.smithy.rust.codegen.core.rustlang.asOptional
import software.amazon.smithy.rust.codegen.core.rustlang.deprecatedShape
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.documentShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.stripOuter
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.eventReceiver
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.forInlineDependency
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.expectRustMetadata
import software.amazon.smithy.rust.codegen.core.smithy.generators.getterName
import software.amazon.smithy.rust.codegen.core.smithy.generators.setterName
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.isRpcBoundProtocol
import software.amazon.smithy.rust.codegen.core.util.outputShape

/**
 * Codegen configuration for the fluent builder generator.
 *
 * Null values indicate "use the default", and defaults are oriented towards
 * the main fluent builders used by the client.
 *
 * These can be overridden for secondary fluent builders, such as used by
 * waiters.
 */
interface FluentBuilderConfig {
    /** Top-level documentation for the builder struct */
    fun documentBuilder(): Writable? = null

    /** `send()` (and friends) method  on the builder */
    fun sendMethods(): Writable? = null

    /** Whether to include paginators or not */
    fun includePaginators(): Boolean = true

    /** Whether to include config override or not */
    fun includeConfigOverride(): Boolean = true
}

private fun FluentBuilderConfig.sendOverridden(): Boolean = sendMethods() != null

/** Default fluent builder generator config */
class DefaultFluentBuilderConfig : FluentBuilderConfig

/**
 * Generates the fluent builders returned by the generated client.
 */
class FluentBuilderGenerator(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
    private val customizations: List<FluentClientCustomization> = emptyList(),
    private val builderName: String = operation.fluentBuilderType(codegenContext.symbolProvider).name,
    private val config: FluentBuilderConfig = DefaultFluentBuilderConfig(),
) {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider
    private val model = codegenContext.model

    private val inputShape = operation.inputShape(model)
    private val inputBuilderType = symbolProvider.symbolForBuilder(inputShape)

    private val outputShape = operation.outputShape(model)
    private val outputType = symbolProvider.toSymbol(outputShape)
    private val errorType = symbolProvider.symbolForOperationError(operation)
    private val operationType = symbolProvider.toSymbol(operation)

    private val scope =
        arrayOf(
            *preludeScope,
            "Arc" to RuntimeType.Arc,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "CustomizableOperation" to
                ClientRustModule.Client.customize.toType()
                    .resolve("CustomizableOperation"),
            "HttpResponse" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("client::orchestrator::HttpResponse"),
            "InputBuilder" to inputBuilderType,
            "Operation" to operationType,
            "OperationError" to errorType,
            "OperationOutput" to outputType,
            "RuntimePlugins" to RuntimeType.runtimePlugins(runtimeConfig),
            "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
            "SdkError" to RuntimeType.sdkError(runtimeConfig),
            "StatusCode" to
                RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                    .resolve("http::StatusCode"),
            "SendResult" to
                ClientRustModule.Client.customize.toType()
                    .resolve("internal::SendResult"),
            "event_receiver" to forInlineDependency(InlineDependency.eventReceiver(runtimeConfig)),
        )

    fun render(writer: RustWriter) {
        if (!config.sendOverridden()) {
            writer.renderInputBuilderImpls()
        }
        writer.renderStruct()
        if (!config.sendOverridden()) {
            writer.renderTraitImpls()
        }
        writer.renderImpl()
    }

    private fun defaultDocumentBuilder(): Writable =
        writable {
            docs("Fluent builder constructing a request to `${operationType.name}`.\n")
            documentShape(operation, model, autoSuppressMissingDocs = false)
        }

    private fun defaultSend(): Writable =
        writable {
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
                    #{epilogue:W}
                }

                /// Consumes this builder, creating a customizable operation that can be modified before being sent.
                pub fn customize(
                    self,
                ) -> #{CustomizableOperation}<#{OperationOutput}, #{OperationError}, Self> {
                    #{CustomizableOperation}::new(self)
                }
                """,
                *scope,
                "epilogue" to (
                    handleEventStreamInitialResponse() ?: writable {
                        rustTemplate("#{Operation}::orchestrate(&runtime_plugins, input).await", *scope)
                    }
                ),
            )
        }

    private fun handleEventStreamInitialResponse(): Writable? {
        if (!codegenContext.protocol.isRpcBoundProtocol) {
            return null
        }

        val eventStreamMember =
            outputShape.members().find { member ->
                member.isEventStream(codegenContext.model)
            }
        if (eventStreamMember == null) {
            return null
        }

        return writable {
            val eventStreamMemberName = symbolProvider.toMemberName(eventStreamMember)
            rustTemplate(
                """
                let mut output =
                    #{Operation}::orchestrate(
                        &runtime_plugins,
                        input,
                    )
                    .await?;

                // Converts any error encountered beyond this point into an `SdkError` response error
                // with an `HttpResponse`. However, since we have already exited the `orchestrate`
                // function, the original `HttpResponse` is no longer available and cannot be restored.
                // This means that header information from the original response has been lost.
                //
                // Note that the response body would have been consumed by the deserializer
                // regardless, even if the initial message was hypothetically processed during
                // the orchestrator's deserialization phase but later resulted in an error.
                fn response_error(
                    err: impl #{Into}<#{BoxError}>
                ) -> #{SdkError}<#{OperationError}, #{HttpResponse}> {
                    #{SdkError}::response_error(err, #{HttpResponse}::new(
                        #{StatusCode}::try_from(200).expect("valid successful code"),
                        #{SdkBody}::empty()))
                }

                let message = output.$eventStreamMemberName.try_recv_initial_response().await.map_err(response_error)?;

                match message {
                    #{Some}(_message) => {
                        #{maybeRecreateOutputWithNonEventStreamMembers:W}
                        #{Ok}(output)
                    }
                    #{None} => #{Ok}(output),
                }
                """,
                *scope,
                "maybeRecreateOutputWithNonEventStreamMembers" to
                    writable {
                        val structuredDataParser = codegenContext.protocolImpl?.structuredDataParser()
                        structuredDataParser?.operationParser(operation)?.also { parser ->
                            rustTemplate(
                                """
                                let mut builder = output.into_builder();
                                builder = #{parser}(
                                    _message.payload(),
                                    builder
                                )
                                .map_err(response_error)?;
                                let output = builder.build().map_err(response_error)?;
                                """,
                                "parser" to parser,
                                *scope,
                            )
                        }
                    },
            )
        }
    }

    private fun RustWriter.renderStruct() {
        // Filter out any derive that isn't Clone. Then add a Debug derive input name
        val derives =
            symbolProvider.toSymbol(inputShape).expectRustMetadata().derives.let { baseDerives ->
                baseDerives.filter { it == RuntimeType.Clone } + RuntimeType.Debug
            }

        (config.documentBuilder() ?: defaultDocumentBuilder())()
        deprecatedShape(operation)
        Attribute(Attribute.derive(derives.toSet())).render(this)
        val configOverride =
            when (config.includeConfigOverride()) {
                true -> "\nconfig_override: #{Option}<crate::config::Builder>,"
                else -> ""
            }
        rustTemplate(
            """
            pub struct $builderName {
                handle: #{Arc}<crate::client::Handle>,
                inner: #{InputBuilder},$configOverride
            }
            """,
            *scope,
        )
    }

    private fun RustWriter.renderImpl() {
        rustBlock("impl $builderName") {
            val configOverride =
                when (config.includeConfigOverride()) {
                    true -> "\nconfig_override: #{None},"
                    else -> ""
                }
            rustTemplate(
                """
                /// Creates a new `$builderName`.
                pub(crate) fn new(handle: #{Arc}<crate::client::Handle>) -> Self {
                    Self {
                        handle,
                        inner: #{Default}::default(),$configOverride
                    }
                }
                """,
                *scope,
            )

            rustTemplate(
                """
                /// Access the ${operationType.name} as a reference.
                pub fn as_input(&self) -> &#{InputBuilder} {
                    &self.inner
                }
                """,
                *scope,
            )

            // Output the send method
            (config.sendMethods() ?: defaultSend())()

            if (config.includeConfigOverride()) {
                rustTemplate(
                    """
                    pub(crate) fn config_override(
                        mut self,
                        config_override: impl #{Into}<crate::config::Builder>,
                    ) -> Self {
                        self.set_config_override(#{Some}(config_override.into()));
                        self
                    }

                    pub(crate) fn set_config_override(
                        &mut self,
                        config_override: #{Option}<crate::config::Builder>,
                    ) -> &mut Self {
                        self.config_override = config_override;
                        self
                    }
                    """,
                    *scope,
                )
            }

            if (config.includePaginators()) {
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
            }

            writeCustomizations(
                customizations,
                FluentClientSection.FluentBuilderImpl(operation, errorType),
            )

            inputShape.members().forEach { member ->
                val memberName = symbolProvider.toMemberName(member)
                // All fields in the builder are optional
                val memberSymbol = symbolProvider.toSymbol(member)
                val outerType = memberSymbol.rustType()
                when (val coreType = outerType.stripOuter<RustType.Option>()) {
                    is RustType.Vec -> renderVecHelper(member, memberName, coreType)
                    is RustType.HashMap -> renderMapHelper(member, memberName, coreType)
                    else -> renderInputHelper(member, memberName, coreType)
                }
                // pure setter
                val setterName = member.setterName()
                val optionalInputType = outerType.asOptional()
                renderInputHelper(member, setterName, optionalInputType)

                val getterName = member.getterName()
                renderGetterHelper(member, getterName, optionalInputType)
            }
        }
    }

    private fun RustWriter.renderTraitImpls() {
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
            *scope,
        )
    }

    private fun RustWriter.renderInputBuilderImpls() {
        FluentClientGenerator.clientOperationFnName(operation, symbolProvider).also { fnName ->
            rustTemplate(
                """
                impl #{InputBuilder} {
                    /// Sends a request with this input using the given client.
                    pub async fn send_with(self, client: &crate::Client) -> #{Result}<
                        #{OperationOutput},
                        #{SdkError}<
                            #{OperationError},
                            #{HttpResponse}
                        >
                    > {
                        let mut fluent_builder = client.$fnName();
                        fluent_builder.inner = self;
                        fluent_builder.send().await
                    }
                }
                """,
                *scope,
            )
        }
    }

    /** Generate and write Rust code for a builder method that sets a Vec<T> */
    private fun RustWriter.renderVecHelper(
        member: MemberShape,
        memberName: String,
        coreType: RustType.Vec,
    ) {
        docs(
            """
            Appends an item to `${member.memberName}`.

            To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).
            """,
        )
        documentShape(member, model)
        deprecatedShape(member)
        coreType.member.asArgument("input").also { input ->
            rust(
                """
                pub fn $memberName(mut self, ${input.argument}) -> Self {
                    self.inner = self.inner.$memberName(${input.value});
                    self
                }
                """,
            )
        }
    }

    /** Generate and write Rust code for a builder method that sets a HashMap<K,V> */
    private fun RustWriter.renderMapHelper(
        member: MemberShape,
        memberName: String,
        coreType: RustType.HashMap,
    ) {
        val k = coreType.key.asArgument("k")
        val v = coreType.member.asArgument("v")

        docs(
            """
            Adds a key-value pair to `${member.memberName}`.

            To override the contents of this collection use [`${member.setterName()}`](Self::${member.setterName()}).
            """,
        )
        documentShape(member, model)
        deprecatedShape(member)
        rust(
            """
            pub fn $memberName(mut self, ${k.argument}, ${v.argument}) -> Self {
                self.inner = self.inner.$memberName(${k.value}, ${v.value});
                self
            }
            """,
        )
    }

    /**
     * Generate and write Rust code for a builder method that sets an input. Can be used for setter methods as well e.g.
     *
     * `renderInputHelper(memberShape, "foo", RustType.String)` -> `pub fn foo(mut self, input: impl Into<String>) -> Self { ... }`
     * `renderInputHelper(memberShape, "set_bar", RustType.Option)` -> `pub fn set_bar(mut self, input: Option<String>) -> Self { ... }`
     */
    private fun RustWriter.renderInputHelper(
        member: MemberShape,
        memberName: String,
        coreType: RustType,
    ) {
        val functionInput = coreType.asArgument("input")

        documentShape(member, model)
        deprecatedShape(member)
        rust(
            """
            pub fn $memberName(mut self, ${functionInput.argument}) -> Self {
                self.inner = self.inner.$memberName(${functionInput.value});
                self
            }
            """,
        )
    }

    /**
     * Generate and write Rust code for a getter method that returns a reference to the inner data.
     */
    private fun RustWriter.renderGetterHelper(
        member: MemberShape,
        memberName: String,
        coreType: RustType,
    ) {
        documentShape(member, model)
        deprecatedShape(member)
        rustTemplate(
            """
            pub fn $memberName(&self) -> &#{CoreType} {
                self.inner.$memberName()
            }
            """,
            "CoreType" to coreType,
        )
    }
}
