/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.HttpBindingIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.HttpTraitBindingGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.util.dq

class AwsRestJsonFactory : ProtocolGeneratorFactory<AwsRestJsonGenerator> {
    override fun build(
        protocolConfig: ProtocolConfig
    ): AwsRestJsonGenerator = with(protocolConfig) {
        AwsRestJsonGenerator(model, symbolProvider, runtimeConfig, writer, operationShape, inputShape)
    }
}

class AwsRestJsonGenerator(
    private val model: Model,
    private val symbolProvider: SymbolProvider,
    private val runtimeConfig: RuntimeConfig,
    private val writer: RustWriter,
    private val operationShape: OperationShape,
    private val inputShape: StructureShape
) : HttpProtocolGenerator(symbolProvider, writer, inputShape) {
    // restJson1 requires all operations to use the HTTP trait
    private val httpTrait = operationShape.expectTrait(HttpTrait::class.java)

    private val httpBindingGenerator = HttpTraitBindingGenerator(
        model,
        symbolProvider,
        runtimeConfig,
        writer,
        operationShape,
        inputShape,
        httpTrait
    )

    private val httpIndex = HttpBindingIndex(model)
    private val requestBuilder = RuntimeType.Http("request::Builder")

    override fun toHttpRequestImpl(implBlockWriter: RustWriter) {
        val contentType = httpIndex.determineRequestContentType(operationShape, "application/json").orElse("application/json")
        httpBindingGenerator.renderUpdateHttpBuilder(implBlockWriter)
        httpBuilderFun(implBlockWriter) {
            write("let builder = \$T::new();", requestBuilder)
            write("let builder = builder.header(\"Content-Type\", ${contentType.dq()});")
            write("self.update_http_builder(builder)")
        }
    }
}
