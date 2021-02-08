/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Derives
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.documentShape
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.config.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.config.Section
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
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
    val protocol: ShapeId
)

interface ProtocolGeneratorFactory<out T : HttpProtocolGenerator> {
    fun buildProtocolGenerator(protocolConfig: ProtocolConfig): T
    fun transformModel(model: Model): Model
    fun symbolProvider(model: Model, base: RustSymbolProvider): RustSymbolProvider = base
    fun support(): ProtocolSupport
}

sealed class OperationSection(name: String) : Section(name) {
    object ImplBlock : OperationSection("ImplBlock")
    object Plugin : OperationSection("Feature")
}

typealias OperationCustomization = NamedSectionGenerator<OperationSection>

/**
 * Abstract class providing scaffolding for HTTP based protocols that must build an HTTP request (headers / URL) and
 * a body.
 */
abstract class HttpProtocolGenerator(
    protocolConfig: ProtocolConfig
) {
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model
    fun renderOperation(
        operationWriter: RustWriter,
        inputWriter: RustWriter,
        operationShape: OperationShape,
        customizations: List<OperationCustomization>
    ) {
        val inputShape = operationShape.inputShape(model)
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        val builderGenerator = OperationInputBuilderGenerator(model, symbolProvider, operationShape, customizations)
        builderGenerator.render(inputWriter)
        // impl OperationInputShape { ... }
        inputWriter.implBlock(inputShape, symbolProvider) {
            toHttpRequestImpl(this, operationShape, inputShape)
            val shapeId = inputShape.expectTrait(SyntheticInputTrait::class.java).body
            val body = shapeId?.let { model.expectShape(it, StructureShape::class.java) }
            toBodyImpl(this, inputShape, body)
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
        Derives(setOf(RuntimeType.Std("clone::Clone"))).render(operationWriter)
        operationWriter.rustBlock("pub struct $operationName") {
            write("input: #T", inputSymbol)
        }
        operationWriter.implBlock(operationShape, symbolProvider) {
            builderGenerator.renderConvenienceMethod(this)

            rustBlock(
                "pub fn build_http_request(&self) -> #T<Vec<u8>>", RuntimeType.Http("request::Request")
            ) {
                write("#T::assemble(self.input.request_builder_base(), self.input.build_body())", inputSymbol)
            }

            fromResponseImpl(this, operationShape)

            rustBlock(
                "pub fn parse_response(&self, response: &#T<impl AsRef<[u8]>>) -> Result<#T, #T>",
                RuntimeType.Http("response::Response"),
                symbolProvider.toSymbol(operationShape.outputShape(model)),
                operationShape.errorSymbol(symbolProvider)
            ) {
                write("Self::from_response(&response)")
            }

            rustBlock("pub fn new(input: #T) -> Self", inputSymbol) {
                write("Self { input }")
            }

            customizations.forEach { customization -> customization.section(OperationSection.ImplBlock)(this) }
        }
        traitImplementations(operationWriter, operationShape)
    }

    protected fun httpBuilderFun(implBlockWriter: RustWriter, f: RustWriter.() -> Unit) {
        implBlockWriter.rustBlock(
            "pub fn request_builder_base(&self) -> #T",
            RuntimeType.HttpRequestBuilder
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
        f: RustWriter.() -> Unit
    ) {
        implBlockWriter.rustBlock(
            "fn from_response(response: &#T<impl AsRef<[u8]>>) -> Result<#T, #T>",
            RuntimeType.Http("response::Response"),
            symbolProvider.toSymbol(operationShape.outputShape(model)),
            operationShape.errorSymbol(symbolProvider)
        ) {
            f(this)
        }
    }

    abstract fun traitImplementations(operationWriter: RustWriter, operationShape: OperationShape)

    abstract fun fromResponseImpl(implBlockWriter: RustWriter, operationShape: OperationShape)

    /**
     * Add necessary methods to the impl block to generate the request body
     *
     * Your implementation MUST call [bodyBuilderFun] to create the public method.
     */
    abstract fun toBodyImpl(implBlockWriter: RustWriter, inputShape: StructureShape, inputBody: StructureShape?)

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
