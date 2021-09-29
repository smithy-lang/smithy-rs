/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.util.inputShape

/**
 * Configuration needed to generate the client for a given Service<->Protocol pair
 */
data class ProtocolConfig(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    val runtimeConfig: RuntimeConfig,
    val serviceShape: ServiceShape,
    val protocol: ShapeId,
    val moduleName: String
)

interface ProtocolGeneratorFactory<out T : HttpProtocolGenerator> {
    fun protocol(protocolConfig: ProtocolConfig): Protocol
    fun buildProtocolGenerator(protocolConfig: ProtocolConfig): T
    fun transformModel(model: Model): Model
    fun symbolProvider(model: Model, base: RustSymbolProvider): RustSymbolProvider = base
    fun support(): ProtocolSupport
}

interface HttpProtocolBodyWriter {
    data class BodyMetadata(val takesOwnership: Boolean)

    fun bodyMetadata(operationShape: OperationShape): BodyMetadata

    fun writeBody(writer: RustWriter, self: String, operationShape: OperationShape)
}

interface HttpProtocolTraitImplWriter {
    fun writeTraitImpls(operationWriter: RustWriter, operationShape: OperationShape)
}

/**
 * Class providing scaffolding for HTTP based protocols that must build an HTTP request (headers / URL) and a body.
 */
abstract class HttpProtocolGenerator(protocolConfig: ProtocolConfig) {
    abstract val makeOperationGenerator: MakeOperationGenerator
    abstract val traitWriter: HttpProtocolTraitImplWriter

    private val runtimeConfig = protocolConfig.runtimeConfig
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model

    private val codegenScope = arrayOf(
        "HttpRequestBuilder" to RuntimeType.HttpRequestBuilder,
        "OpBuildError" to protocolConfig.runtimeConfig.operationBuildError(),
        "Request" to RuntimeType.Http("request::Request"),
        "RequestBuilder" to RuntimeType.HttpRequestBuilder,
        "SdkBody" to RuntimeType.sdkBody(protocolConfig.runtimeConfig),
        "config" to RuntimeType.Config,
        "header_util" to CargoDependency.SmithyHttp(protocolConfig.runtimeConfig).asType().member("header"),
        "http" to RuntimeType.http,
        "operation" to RuntimeType.operationModule(runtimeConfig),
    )

    /** Write code into the impl block for [operationShape] */
    open fun operationImplBlock(implBlockWriter: RustWriter, operationShape: OperationShape) {}

    fun renderOperation(
        operationWriter: RustWriter,
        inputWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>
    ) {
        val inputShape = operationShape.inputShape(model)
        val builderGenerator = BuilderGenerator(model, symbolProvider, operationShape.inputShape(model))
        builderGenerator.render(inputWriter)

        // TODO: One day, it should be possible for callers to invoke
        // buildOperationType* directly to get the type rather than depending
        // on these aliases.
        val operationTypeOutput = buildOperationTypeOutput(inputWriter, operationShape)
        val operationTypeRetry = buildOperationTypeRetry(inputWriter, customizations)
        val inputPrefix = symbolProvider.toSymbol(inputShape).name
        inputWriter.rust(
            """
            ##[doc(hidden)] pub type ${inputPrefix}OperationOutputAlias = $operationTypeOutput;
            ##[doc(hidden)] pub type ${inputPrefix}OperationRetryAlias = $operationTypeRetry;
            """
        )

        // impl OperationInputShape { ... }
        val operationName = symbolProvider.toSymbol(operationShape).name
        inputWriter.implBlock(inputShape, symbolProvider) {
            writeCustomizations(customizations, OperationSection.InputImpl(customizations, operationShape, inputShape))
            makeOperationGenerator.generateMakeOperation(this, operationShape, customizations)
            rustBlockTemplate(
                "fn assemble(mut builder: #{RequestBuilder}, body: #{SdkBody}) -> #{Request}<#{SdkBody}>",
                *codegenScope
            ) {
                rustTemplate(
                    """
                    if let Some(content_length) = body.content_length() {
                        builder = #{header_util}::set_header_if_absent(
                                    builder,
                                    #{http}::header::CONTENT_LENGTH,
                                    content_length
                        );
                    }
                    builder.body(body).expect("should be valid request")
                    """,
                    *codegenScope
                )
            }

            // pub fn builder() -> ... { }
            builderGenerator.renderConvenienceMethod(this)
        }

        // pub struct Operation { ... }
        operationWriter.documentShape(operationShape, model)
        Attribute.Derives(setOf(RuntimeType.Clone, RuntimeType.Default, RuntimeType.Debug)).render(operationWriter)
        operationWriter.rustBlock("pub struct $operationName") {
            write("_private: ()")
        }
        operationWriter.implBlock(operationShape, symbolProvider) {
            builderGenerator.renderConvenienceMethod(this)

            operationImplBlock(this, operationShape)

            rustBlock("pub fn new() -> Self") {
                rust("Self { _private: () }")
            }

            writeCustomizations(customizations, OperationSection.OperationImplBlock(customizations))
        }
        traitWriter.writeTraitImpls(operationWriter, operationShape)
    }

    private fun buildOperationTypeOutput(writer: RustWriter, shape: OperationShape): String =
        writer.format(symbolProvider.toSymbol(shape))

    private fun buildOperationTypeRetry(writer: RustWriter, customizations: List<OperationCustomization>): String =
        customizations.mapNotNull { it.retryType() }.firstOrNull()?.let { writer.format(it) } ?: "()"
}
