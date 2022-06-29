/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.protocol

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlockTemplate
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.smithy.generators.http.RequestBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.protocols.HttpLocation
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.findStreamingMember
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.inputShape

/** Generates the `make_operation` function on input structs */
open class MakeOperationGenerator(
    protected val coreCodegenContext: CoreCodegenContext,
    private val protocol: Protocol,
    private val bodyGenerator: ProtocolPayloadGenerator,
    private val public: Boolean,
    /** Whether or not to include default values for content-length and content-type */
    private val includeDefaultPayloadHeaders: Boolean,
    private val functionName: String = "make_operation",
) {
    protected val model = coreCodegenContext.model
    protected val runtimeConfig = coreCodegenContext.runtimeConfig
    protected val symbolProvider = coreCodegenContext.symbolProvider
    protected val httpBindingResolver = protocol.httpBindingResolver

    private val sdkId =
        coreCodegenContext.serviceShape.getTrait<ServiceTrait>()?.sdkId?.lowercase()?.replace(" ", "")
            ?: coreCodegenContext.serviceShape.id.getName(coreCodegenContext.serviceShape)

    private val codegenScope = arrayOf(
        "config" to RuntimeType.Config,
        "header_util" to CargoDependency.SmithyHttp(runtimeConfig).asType().member("header"),
        "http" to RuntimeType.http,
        "HttpRequestBuilder" to RuntimeType.HttpRequestBuilder,
        "OpBuildError" to coreCodegenContext.runtimeConfig.operationBuildError(),
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "SdkBody" to RuntimeType.sdkBody(coreCodegenContext.runtimeConfig)
    )

    fun generateMakeOperation(
        implBlockWriter: RustWriter,
        shape: OperationShape,
        customizations: List<OperationCustomization>,
    ) {
        val operationName = symbolProvider.toSymbol(shape).name
        val baseReturnType = buildOperationType(implBlockWriter, shape, customizations)
        val returnType = "std::result::Result<$baseReturnType, ${implBlockWriter.format(runtimeConfig.operationBuildError())}>"
        val outputSymbol = symbolProvider.toSymbol(shape)

        val takesOwnership = bodyGenerator.payloadMetadata(shape).takesOwnership
        val mut = customizations.any { it.mutSelf() }
        val consumes = customizations.any { it.consumesSelf() } || takesOwnership
        val self = "self".letIf(mut) { "mut $it" }.letIf(!consumes) { "&$it" }
        val fnType = if (public) "pub async fn" else "async fn"

        implBlockWriter.docs("Consumes the builder and constructs an Operation<#D>", outputSymbol)
        Attribute.AllowUnusedMut.render(implBlockWriter) // For codegen simplicity
        Attribute.Custom("allow(clippy::let_and_return)").render(implBlockWriter) // For codegen simplicity, allow `let x = ...; x`
        Attribute.Custom("allow(clippy::needless_borrow)").render(implBlockWriter) // Allows builders that don’t consume the input borrow
        implBlockWriter.rustBlockTemplate(
            "$fnType $functionName($self, _config: &#{config}::Config) -> $returnType",
            *codegenScope
        ) {
            writeCustomizations(customizations, OperationSection.MutateInput(customizations, "self", "_config"))

            withBlock("let mut request = {", "};") {
                createHttpRequest(this, shape)
            }
            rust("let mut properties = aws_smithy_http::property_bag::SharedPropertyBag::new();")

            // When the payload is a `ByteStream`, `into_inner()` already returns an `SdkBody`, so we mute this
            // Clippy warning to make the codegen a little simpler in that case.
            Attribute.Custom("allow(clippy::useless_conversion)").render(this)
            withBlockTemplate("let body = #{SdkBody}::from(", ");", *codegenScope) {
                bodyGenerator.generatePayload(this, "self", shape)
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
                    if let Some(content_length) = body.content_length() {
                        request = #{header_util}::set_request_header_if_absent(request, #{http}::header::CONTENT_LENGTH, content_length);
                    }
                    """,
                    *codegenScope
                )
            }
            rust("""let request = request.body(body).expect("should be valid request");""")
            rustTemplate(
                """
                let mut request = #{operation}::Request::from_parts(request, properties);
                """,
                *codegenScope
            )
            writeCustomizations(customizations, OperationSection.MutateRequest(customizations, "request", "_config"))
            rustTemplate(
                """
                let op = #{operation}::Operation::new(request, #{OperationType}::new())
                    .with_metadata(#{operation}::Metadata::new(${operationName.dq()}, ${sdkId.dq()}));
                """,
                *codegenScope,
                "OperationType" to symbolProvider.toSymbol(shape)
            )
            writeCustomizations(customizations, OperationSection.FinalizeOperation(customizations, "op", "_config"))
            rust("Ok(op)")
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
        customizations.mapNotNull { it.retryType() }.firstOrNull()?.let { writer.format(it) } ?: "()"

    private fun needsContentLength(operationShape: OperationShape): Boolean {
        return protocol.httpBindingResolver.requestBindings(operationShape)
            .any { it.location == HttpLocation.DOCUMENT || it.location == HttpLocation.PAYLOAD }
    }

    open fun createHttpRequest(writer: RustWriter, operationShape: OperationShape) {
        val httpBindingGenerator = RequestBindingGenerator(
            coreCodegenContext,
            protocol,
            operationShape
        )
        val contentType = httpBindingResolver.requestContentType(operationShape)
        httpBindingGenerator.renderUpdateHttpBuilder(writer)

        writer.rust("let mut builder = update_http_builder(&self, #T::new())?;", RuntimeType.HttpRequestBuilder)
        if (includeDefaultPayloadHeaders && contentType != null) {
            writer.rustTemplate(
                "builder = #{header_util}::set_request_header_if_absent(builder, #{http}::header::CONTENT_TYPE, ${contentType.dq()});",
                *codegenScope
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
                *codegenScope
            )
        }
        writer.rust("builder")
    }
}
