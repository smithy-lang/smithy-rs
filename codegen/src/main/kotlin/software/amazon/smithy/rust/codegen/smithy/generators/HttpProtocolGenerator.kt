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
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.documentShape
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait

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

/**
 * Abstract class providing scaffolding for HTTP based protocols that must build an HTTP request (headers / URL) and
 * a body.
 */
abstract class HttpProtocolGenerator(
    protocolConfig: ProtocolConfig
) {
    private val symbolProvider = protocolConfig.symbolProvider
    private val model = protocolConfig.model
    fun renderOperation(writer: RustWriter, operationShape: OperationShape) {
        val inputShape = model.expectShape(operationShape.input.get(), StructureShape::class.java)
        val outputShape = model.expectShape(operationShape.output.get(), StructureShape::class.java)
        val inputSymbol = symbolProvider.toSymbol(inputShape)
        // val builderGenerator = BuilderGenerator(model, symbolProvider, writer, inputShape, operationShape)
        writer.rustBlock("impl ${symbolProvider.toSymbol(inputShape).name}") {
            toHttpRequestImpl(this, operationShape, inputShape)
            val shapeId = inputShape.expectTrait(SyntheticInputTrait::class.java).body
            val body = shapeId?.let { model.expectShape(it, StructureShape::class.java) }
            toBodyImpl(this, inputShape, body)
            // TODO: streaming shapes need special support
            rustBlock(
                "pub fn assemble(builder: \$T, body: Vec<u8>) -> \$T<Vec<u8>>",
                RuntimeType.HttpRequestBuilder,
                RuntimeType.Http("request::Request")
            ) {
                write("builder.header(\$T, body.len()).body(body)", RuntimeType.Http("header::CONTENT_LENGTH"))
                write(""".expect("http request should be valid")""")
            }
            // builderGenerator.convenienceMethod(this)
        }

        val operationName = symbolProvider.toSymbol(operationShape).name
        writer.documentShape(operationShape, model)
        writer.rustBlock("pub struct $operationName") {
            write("input: \$T", inputSymbol)
        }

        val outputSymbol = symbolProvider.toSymbol(outputShape)
        val errorSymbol = operationShape.errorSymbol(symbolProvider)

        writer.rustBlock("impl $operationName") {
            rustBlock(
                "pub fn from_response(response: \$T<impl AsRef<[u8]>>) -> Result<\$T, \$T>",
                RuntimeType.Http("response::Response"),
                outputSymbol,
                errorSymbol
            ) {
                fromResponse(this, operationShape)
            }

            rustBlock(
                "pub fn to_http_request(&self) -> \$T<Vec<u8>>", RuntimeType.Http("request::Request")
            ) {
                write("\$T::assemble(self.input.request_builder_base(), self.input.build_body())", inputSymbol)
            }

            rustBlock("pub fn new(input: \$T) -> Self", inputSymbol) {
                write("Self { input }")
            }

            val builderSymbol = inputShape.builderSymbol(symbolProvider)
            rustBlock("pub fn builder() -> \$T", inputShape.builderSymbol(symbolProvider)) {
                write("\$T::default()", builderSymbol)
            }
        }
    }
    abstract fun fromResponse(
        writer: RustWriter,
        operationShape: OperationShape
    )

    protected fun httpBuilderFun(implBlockWriter: RustWriter, f: RustWriter.() -> Unit) {
        implBlockWriter.rustBlock(
            "pub fn request_builder_base(&self) -> \$T",
            RuntimeType.HttpRequestBuilder
        ) {
            f(this)
        }
    }

    protected fun bodyBuilderFun(implBlockWriter: RustWriter, f: RustWriter.() -> Unit) {
        implBlockWriter.rustBlock(
            "pub fn build_body(&self) -> Vec<u8>"
        ) {
            f(this)
        }
    }

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
