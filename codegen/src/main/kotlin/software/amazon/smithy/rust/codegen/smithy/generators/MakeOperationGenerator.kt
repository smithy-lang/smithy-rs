/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.getTrait

/** Generates the `make_operation` function on input structs */
class MakeOperationGenerator(
    protocolConfig: ProtocolConfig,
    private val bodyWriter: HttpProtocolBodyWriter,
) {
    private val runtimeConfig = protocolConfig.runtimeConfig
    private val symbolProvider = protocolConfig.symbolProvider

    private val sdkId =
        protocolConfig.serviceShape.getTrait<ServiceTrait>()?.sdkId?.toLowerCase()?.replace(" ", "")
            ?: protocolConfig.serviceShape.id.getName(protocolConfig.serviceShape)

    private val codegenScope = arrayOf(
        "SdkBody" to RuntimeType.sdkBody(protocolConfig.runtimeConfig),
        "config" to RuntimeType.Config,
        "operation" to RuntimeType.operationModule(runtimeConfig),
    )

    fun generateMakeOperation(
        implBlockWriter: RustWriter,
        shape: OperationShape,
        operationName: String,
        customizations: List<OperationCustomization>
    ) {
        val baseReturnType = buildOperationType(implBlockWriter, shape, customizations)
        val returnType = "std::result::Result<$baseReturnType, ${implBlockWriter.format(runtimeConfig.operationBuildError())}>"
        val outputSymbol = symbolProvider.toSymbol(shape)

        val takesOwnership = bodyWriter.bodyMetadata(shape).takesOwnership
        val mut = customizations.any { it.mutSelf() }
        val consumes = customizations.any { it.consumesSelf() } || takesOwnership
        val self = "self".letIf(mut) { "mut $it" }.letIf(!consumes) { "&$it" }

        implBlockWriter.docs("Consumes the builder and constructs an Operation<#D>", outputSymbol)
        implBlockWriter.rust("##[allow(clippy::let_and_return)]") // For codegen simplicity, allow `let x = ...; x`
        implBlockWriter.rustBlockTemplate(
            "pub fn make_operation($self, _config: &#{config}::Config) -> $returnType",
            *codegenScope
        ) {
            writeCustomizations(customizations, OperationSection.MutateInput("self", "_config"))
            rust("let properties = smithy_http::property_bag::SharedPropertyBag::new();")
            rust("let request = self.request_builder_base()?;")
            withBlock("let body =", ";") {
                bodyWriter.writeBody(this, "self", shape)
            }
            rust("let request = Self::assemble(request, body);")
            rustTemplate(
                """
                ##[allow(unused_mut)]
                let mut request = #{operation}::Request::from_parts(request.map(#{SdkBody}::from), properties);
                """,
                *codegenScope
            )
            writeCustomizations(customizations, OperationSection.MutateRequest("request", "_config"))
            rustTemplate(
                """
                let op = #{operation}::Operation::new(request, #{OperationType}::new())
                    .with_metadata(#{operation}::Metadata::new(${operationName.dq()}, ${sdkId.dq()}));
                """,
                *codegenScope,
                "OperationType" to symbolProvider.toSymbol(shape)
            )
            writeCustomizations(customizations, OperationSection.FinalizeOperation("op", "_config"))
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
}
