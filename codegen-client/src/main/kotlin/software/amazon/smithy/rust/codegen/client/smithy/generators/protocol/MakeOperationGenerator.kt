/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.protocol

import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.http.RequestBindingGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientAdditionalPayloadContext
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolPayloadGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.findStreamingMember
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.sdkId

// TODO(enableNewSmithyRuntimeCleanup): Delete this class when cleaning up `enableNewSmithyRuntime`
/** Generates the `make_operation` function on input structs */
open class MakeOperationGenerator(
    protected val codegenContext: CodegenContext,
    private val protocol: Protocol,
    private val bodyGenerator: ProtocolPayloadGenerator,
    private val public: Boolean,
    /** Whether to include default values for content-length and content-type */
    private val includeDefaultPayloadHeaders: Boolean,
    private val functionName: String = "make_operation",
) {
    protected val model = codegenContext.model
    protected val runtimeConfig = codegenContext.runtimeConfig
    protected val symbolProvider = codegenContext.symbolProvider
    private val httpBindingResolver = protocol.httpBindingResolver
    private val defaultClassifier = RuntimeType.smithyHttp(runtimeConfig)
        .resolve("retry::DefaultResponseRetryClassifier")

    private val sdkId = codegenContext.serviceShape.sdkId()

    private val codegenScope = arrayOf(
        *preludeScope,
        "config" to ClientRustModule.config,
        "header_util" to RuntimeType.smithyHttp(runtimeConfig).resolve("header"),
        "http" to RuntimeType.Http,
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "HttpRequestBuilder" to RuntimeType.HttpRequestBuilder,
        "OpBuildError" to runtimeConfig.operationBuildError(),
        "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
        "SharedPropertyBag" to RuntimeType.smithyHttp(runtimeConfig).resolve("property_bag::SharedPropertyBag"),
        "RetryMode" to RuntimeType.smithyTypes(runtimeConfig).resolve("retry::RetryMode"),
    )

    fun generateMakeOperation(
        implBlockWriter: RustWriter,
        shape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val operationName = symbolProvider.toSymbol(shape).name
        val baseReturnType = buildOperationType(implBlockWriter, shape, customizations)
        val returnType =
            "#{Result}<$baseReturnType, ${implBlockWriter.format(runtimeConfig.operationBuildError())}>"
        val outputSymbol = symbolProvider.toSymbol(shape)

        val takesOwnership = bodyGenerator.payloadMetadata(shape).takesOwnership
        val mut = customizations.any { it.mutSelf() }
        val consumes = customizations.any { it.consumesSelf() } || takesOwnership
        val self = "self".letIf(mut) { "mut $it" }.letIf(!consumes) { "&$it" }
        val fnType = if (public) "pub async fn" else "async fn"

        implBlockWriter.docs("Consumes the builder and constructs an Operation<#D>", outputSymbol)
        // For codegen simplicity
        Attribute.AllowUnusedMut.render(implBlockWriter)
        // For codegen simplicity, allow `let x = ...; x`
        Attribute.AllowClippyLetAndReturn.render(implBlockWriter)
        // Allows builders that don’t consume the input borrow
        Attribute.AllowClippyNeedlessBorrow.render(implBlockWriter)

        implBlockWriter.rustBlockTemplate(
            "$fnType $functionName($self, _config: &#{config}::Config) -> $returnType",
            *codegenScope,
        ) {
            rustTemplate(
                """
                assert_ne!(_config.retry_config().map(|rc| rc.mode()), #{Option}::Some(#{RetryMode}::Adaptive), "Adaptive retry mode is unsupported, please use Standard mode or disable retries.");
                """,
                *codegenScope,
            )
            writeCustomizations(customizations, OperationSection.MutateInput(customizations, "self", "_config"))

            withBlock("let mut request = {", "};") {
                createHttpRequest(this, shape)
            }
            rustTemplate("let mut properties = #{SharedPropertyBag}::new();", *codegenScope)

            // When the payload is a `ByteStream`, `into_inner()` already returns an `SdkBody`, so we mute this
            // Clippy warning to make the codegen a little simpler in that case.
            Attribute.AllowClippyUselessConversion.render(this)
            withBlockTemplate("let body = #{SdkBody}::from(", ");", *codegenScope) {
                bodyGenerator.generatePayload(
                    this,
                    "self",
                    shape,
                    ClientAdditionalPayloadContext(propertyBagAvailable = true),
                )
                val streamingMember = shape.inputShape(model).findStreamingMember(model)
                val isBlobStreaming = streamingMember != null && model.expectShape(streamingMember.target) is BlobShape
                if (isBlobStreaming) {
                    // Consume the `ByteStream` into its inner `SdkBody`.
                    rust(".into_inner()")
                }
            }
            if (includeDefaultPayloadHeaders && needsContentLength(shape)) {
                rustTemplate(
                    """
                    if let #{Some}(content_length) = body.content_length() {
                        request = #{header_util}::set_request_header_if_absent(request, #{http}::header::CONTENT_LENGTH, content_length);
                    }
                    """,
                    *codegenScope,
                )
            }
            rust("""let request = request.body(body).expect("should be valid request");""")
            rustTemplate(
                """
                let mut request = #{operation}::Request::from_parts(request, properties);
                """,
                *codegenScope,
            )
            writeCustomizations(customizations, OperationSection.MutateRequest(customizations, "request", "_config"))
            rustTemplate(
                """
                let op = #{operation}::Operation::new(request, #{OperationType}::new())
                    .with_metadata(#{operation}::Metadata::new(${operationName.dq()}, ${sdkId.dq()}));
                """,
                *codegenScope,
                "OperationType" to symbolProvider.toSymbol(shape),
            )
            writeCustomizations(customizations, OperationSection.FinalizeOperation(customizations, "op", "_config"))
            rustTemplate("#{Ok}(op)", *codegenScope)
        }
    }

    private fun buildOperationType(
        writer: RustWriter,
        shape: OperationShape,
        customizations: List<OperationCustomization>,
    ): String {
        val operationT = RuntimeType.operation(runtimeConfig)
        val output = buildOperationTypeOutput(writer, shape)
        val retry = buildOperationTypeRetry(writer, customizations)
        return with(writer) { "${format(operationT)}<$output, $retry>" }
    }

    private fun buildOperationTypeOutput(writer: RustWriter, shape: OperationShape): String =
        writer.format(symbolProvider.toSymbol(shape))

    private fun buildOperationTypeRetry(writer: RustWriter, customizations: List<OperationCustomization>): String =
        (customizations.firstNotNullOfOrNull { it.retryType() } ?: defaultClassifier).let { writer.format(it) }

    private fun needsContentLength(operationShape: OperationShape): Boolean {
        return protocol.httpBindingResolver.requestBindings(operationShape)
            .any { it.location == HttpLocation.DOCUMENT || it.location == HttpLocation.PAYLOAD }
    }

    open fun createHttpRequest(writer: RustWriter, operationShape: OperationShape) {
        val httpBindingGenerator = RequestBindingGenerator(
            codegenContext,
            protocol,
            operationShape,
        )
        val contentType = httpBindingResolver.requestContentType(operationShape)
        httpBindingGenerator.renderUpdateHttpBuilder(writer)

        writer.rust("let mut builder = update_http_builder(&self, #T::new())?;", RuntimeType.HttpRequestBuilder)
        if (includeDefaultPayloadHeaders && contentType != null) {
            writer.rustTemplate(
                "builder = #{header_util}::set_request_header_if_absent(builder, #{http}::header::CONTENT_TYPE, ${contentType.dq()});",
                *codegenScope,
            )
        }
        for (header in protocol.additionalRequestHeaders(operationShape)) {
            writer.rustTemplate(
                """
                builder = #{header_util}::set_request_header_if_absent(
                    builder,
                    #{http}::header::HeaderName::from_static(${header.first.dq()}),
                    ${header.second.dq()}
                );
                """,
                *codegenScope,
            )
        }
        writer.rust("builder")
    }
}
