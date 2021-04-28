/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.withBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.error.errorSymbol
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.outputShape

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
    fun buildProtocolGenerator(protocolConfig: ProtocolConfig): T
    fun transformModel(model: Model): Model
    fun symbolProvider(model: Model, base: RustSymbolProvider): RustSymbolProvider = base
    fun support(): ProtocolSupport
}

/**
 * Abstract class providing scaffolding for HTTP based protocols that must build an HTTP request (headers / URL) and
 * a body.
 */
abstract class HttpProtocolGenerator(
    private val protocolConfig: ProtocolConfig
) {
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model
    private val buildErrorT = protocolConfig.runtimeConfig.operationBuildError()
    fun renderOperation(
        operationWriter: RustWriter,
        inputWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>
    ) {
        /* if (operationShape.hasTrait(EndpointTrait::class.java)) {
            TODO("https://github.com/awslabs/smithy-rs/issues/197")
        } */
        val inputShape = operationShape.inputShape(model)
        val sdkId =
            protocolConfig.serviceShape.getTrait(ServiceTrait::class.java)
                .map { it.sdkId.toLowerCase().replace(" ", "") }.orElse(protocolConfig.serviceShape.id.name)
        val builderGenerator = BuilderGenerator(model, symbolProvider, operationShape.inputShape(model))
        builderGenerator.render(inputWriter)
        // impl OperationInputShape { ... }

        inputWriter.implBlock(inputShape, symbolProvider) {
            buildOperation(this, operationShape, customizations, sdkId)
            toHttpRequestImpl(this, operationShape, inputShape)
            val shapeId = inputShape.expectTrait(SyntheticInputTrait::class.java).body
            val body = shapeId?.let { model.expectShape(it, StructureShape::class.java) }
            toBodyImpl(this, inputShape, body, operationShape)
            // TODO: streaming shapes need special support
            rustBlock(
                "pub fn assemble(builder: #1T, body: #3T) -> #2T<#3T>",
                RuntimeType.HttpRequestBuilder,
                RuntimeType.Http("request::Request"),
                RuntimeType.ByteSlab
            ) {
                write("builder.header(#T, body.len()).body(body)", RuntimeType.Http("header::CONTENT_LENGTH"))
                write(""".expect("http request should be valid")""")
            }

            // pub fn builder() -> ... { }
            builderGenerator.renderConvenienceMethod(this)
        }
        val operationName = symbolProvider.toSymbol(operationShape).name
        operationWriter.documentShape(operationShape, model)
        Attribute.Derives(setOf(RuntimeType.Clone)).render(operationWriter)
        operationWriter.rustBlock("pub struct $operationName") {
            write("_private: ()")
        }
        operationWriter.implBlock(operationShape, symbolProvider) {
            builderGenerator.renderConvenienceMethod(this)

            fromResponseImpl(this, operationShape)

            rustBlock(
                "pub fn parse_response(&self, response: &#T<impl AsRef<[u8]>>) -> Result<#T, #T>",
                RuntimeType.Http("response::Response"),
                symbolProvider.toSymbol(operationShape.outputShape(model)),
                operationShape.errorSymbol(symbolProvider)
            ) {
                write("Self::from_response(&response)")
            }

            rustBlock("pub fn new() -> Self") {
                rust("Self { _private: () }")
            }

            customizations.forEach { customization -> customization.section(OperationSection.OperationImplBlock)(this) }
        }
        traitImplementations(operationWriter, operationShape)
    }

    protected fun httpBuilderFun(implBlockWriter: RustWriter, f: RustWriter.() -> Unit) {
        implBlockWriter.rustBlock(
            "pub fn request_builder_base(&self) -> Result<#T, #T>",
            RuntimeType.HttpRequestBuilder, buildErrorT
        ) {
            f(this)
        }
    }

    protected fun bodyBuilderFun(implBlockWriter: RustWriter, f: RustWriter.() -> Unit) {
        implBlockWriter.rustBlock(
            "pub fn build_body(&self) -> #T", RuntimeType.ByteSlab
        ) {
            f(this)
        }
    }

    protected fun fromResponseFun(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        block: RustWriter.() -> Unit
    ) {
        Attribute.Custom("allow(clippy::unnecessary_wraps)").render(implBlockWriter)
        implBlockWriter.rustBlock(
            "fn from_response(response: &#T<impl AsRef<[u8]>>) -> Result<#T, #T>",
            RuntimeType.Http("response::Response"),
            symbolProvider.toSymbol(operationShape.outputShape(model)),
            operationShape.errorSymbol(symbolProvider)
        ) {
            block(this)
        }
    }

    private fun buildOperation(implBlockWriter: RustWriter, shape: OperationShape, features: List<OperationCustomization>, sdkId: String) {
        val runtimeConfig = protocolConfig.runtimeConfig
        val outputSymbol = symbolProvider.toSymbol(shape)
        val operationT = RuntimeType.operation(runtimeConfig)
        val operationModule = RuntimeType.operationModule(runtimeConfig)
        val sdkBody = RuntimeType.sdkBody(runtimeConfig)
        val retryType = features.mapNotNull { it.retryType() }.firstOrNull()?.let { implBlockWriter.format(it) } ?: "()"

        val baseReturnType = with(implBlockWriter) { "${format(operationT)}<${format(outputSymbol)}, $retryType>" }
        val returnType = "Result<$baseReturnType, ${implBlockWriter.format(runtimeConfig.operationBuildError())}>"

        implBlockWriter.docs("Consumes the builder and constructs an Operation<#D>", outputSymbol)
        // For codegen simplicity, allow `let x = ...; x`
        implBlockWriter.rust("##[allow(clippy::let_and_return)]")
        val mut = features.any { it.mutSelf() }
        val consumes = features.any { it.consumesSelf() }
        val self = "self".letIf(mut) { "mut $it" }.letIf(!consumes) { "&$it" }
        implBlockWriter.rustBlock("pub fn make_operation($self, _config: &#T::Config) -> $returnType", RuntimeType.Config) {
            withBlock("Ok({", "})") {
                features.forEach { it.section(OperationSection.MutateInput("self", "_config"))(this) }
                rust("let request = Self::assemble(self.request_builder_base()?, self.build_body());")
                rust(
                    """
                    ##[allow(unused_mut)]
                    let mut request = #T::Request::new(request.map(#T::from));
                """,
                    operationModule, sdkBody
                )
                features.forEach { it.section(OperationSection.MutateRequest("request", "_config"))(this) }
                rust(
                    """
                    let op = #1T::Operation::new(
                        request,
                        #2T::new()
                    ).with_metadata(#1T::Metadata::new(${shape.id.name.dq()}, ${sdkId.dq()}));
                """,
                    operationModule, symbolProvider.toSymbol(shape)
                )
                features.forEach { it.section(OperationSection.FinalizeOperation("op", "_config"))(this) }
                rust("op")
            }
        }
    }

    abstract fun traitImplementations(operationWriter: RustWriter, operationShape: OperationShape)

    abstract fun fromResponseImpl(implBlockWriter: RustWriter, operationShape: OperationShape)

    /**
     * Add necessary methods to the impl block to generate the request body
     *
     * Your implementation MUST call [bodyBuilderFun] to create the public method.
     */
    abstract fun toBodyImpl(
        implBlockWriter: RustWriter,
        inputShape: StructureShape,
        inputBody: StructureShape?,
        operationShape: OperationShape
    )

    /**
     * Add necessary methods to the impl block for the input shape.
     *
     * Your implementation MUST call [httpBuilderFun] to create the public method.
     */
    abstract fun toHttpRequestImpl(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    )
}
