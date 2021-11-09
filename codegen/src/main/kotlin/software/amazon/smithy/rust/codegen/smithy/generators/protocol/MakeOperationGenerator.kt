/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.protocol

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.smithy.generators.http.RequestBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.inputShape

/** Generates the `make_operation` function on input structs */
open class MakeOperationGenerator(
    protected val codegenContext: CodegenContext,
    private val protocol: Protocol,
    private val bodyGenerator: ProtocolBodyGenerator,
    private val functionName: String = "make_operation",
    private val public: Boolean = true
) {
    protected val runtimeConfig = codegenContext.runtimeConfig
    protected val symbolProvider = codegenContext.symbolProvider
    protected val httpBindingResolver = protocol.httpBindingResolver

    private val sdkId =
        codegenContext.serviceShape.getTrait<ServiceTrait>()?.sdkId?.toLowerCase()?.replace(" ", "")
            ?: codegenContext.serviceShape.id.getName(codegenContext.serviceShape)

    private val codegenScope = arrayOf(
        "config" to RuntimeType.Config,
        "header_util" to CargoDependency.SmithyHttp(runtimeConfig).asType().member("header"),
        "http" to RuntimeType.http,
        "HttpRequestBuilder" to RuntimeType.HttpRequestBuilder,
        "OpBuildError" to codegenContext.runtimeConfig.operationBuildError(),
        "operation" to RuntimeType.operationModule(runtimeConfig),
        "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
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

        val takesOwnership = bodyGenerator.bodyMetadata(shape).takesOwnership
        val mut = customizations.any { it.mutSelf() }
        val consumes = customizations.any { it.consumesSelf() } || takesOwnership
        val self = "self".letIf(mut) { "mut $it" }.letIf(!consumes) { "&$it" }
        val fnType = if (public) "pub async fn" else "async fn"

        implBlockWriter.docs("Consumes the builder and constructs an Operation<#D>", outputSymbol)
        Attribute.Custom("allow(clippy::let_and_return)").render(implBlockWriter) // For codegen simplicity, allow `let x = ...; x`
        Attribute.Custom("allow(clippy::needless_borrow)").render(implBlockWriter) // Allows builders that don’t consume the input borrow
        implBlockWriter.rustBlockTemplate(
            "$fnType $functionName($self, _config: &#{config}::Config) -> $returnType",
            *codegenScope
        ) {
            generateRequestBuilderBaseFn(this, shape)
            writeCustomizations(customizations, OperationSection.MutateInput(customizations, "self", "_config"))
            rust("let properties = aws_smithy_http::property_bag::SharedPropertyBag::new();")
            rust("let request = request_builder_base(&self)?;")
            withBlock("let body =", ";") {
                bodyGenerator.generateBody(this, "self", shape)
            }
            rust("let request = Self::assemble(request, body);")
            rustTemplate(
                """
                ##[allow(unused_mut)]
                let mut request = #{operation}::Request::from_parts(request.map(#{SdkBody}::from), properties);
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

    protected fun RustWriter.inRequestBuilderBaseFn(inputShape: StructureShape, f: RustWriter.() -> Unit) {
        Attribute.Custom("allow(clippy::unnecessary_wraps)").render(this)
        rustBlockTemplate(
            "fn request_builder_base(input: &#{Input}) -> std::result::Result<#{HttpRequestBuilder}, #{OpBuildError}>",
            *codegenScope,
            "Input" to symbolProvider.toSymbol(inputShape)
        ) {
            f(this)
        }
    }

    open fun generateRequestBuilderBaseFn(writer: RustWriter, operationShape: OperationShape) {
        val inputShape = operationShape.inputShape(codegenContext.model)
        val httpBindingGenerator = RequestBindingGenerator(
            codegenContext,
            protocol.defaultTimestampFormat,
            httpBindingResolver,
            operationShape,
            inputShape,
        )
        val contentType = httpBindingResolver.requestContentType(operationShape)
        httpBindingGenerator.renderUpdateHttpBuilder(writer)
        writer.inRequestBuilderBaseFn(inputShape) {
            Attribute.AllowUnusedMut.render(this)
            writer.rust("let mut builder = update_http_builder(input, #T::new())?;", RuntimeType.HttpRequestBuilder)
            val additionalHeaders = listOfNotNull(contentType?.let { "content-type" to it }) + protocol.additionalHeaders(operationShape)
            for (header in additionalHeaders) {
                writer.rustTemplate(
                    """
                    builder = #{header_util}::set_header_if_absent(
                        builder,
                        #{http}::header::HeaderName::from_static(${header.first.dq()}),
                        ${header.second.dq()}
                    );
                    """,
                    *codegenScope
                )
            }
            rust("Ok(builder)")
        }
    }
}
