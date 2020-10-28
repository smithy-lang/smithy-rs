/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory

class AwsJson10Factory : ProtocolGeneratorFactory<AwsJson10Generator> {
    override fun build(
        model: Model,
        symbolProvider: SymbolProvider,
        runtimeConfig: RuntimeConfig,
        writer: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ): AwsJson10Generator = AwsJson10Generator(symbolProvider, writer, operationShape, inputShape)
}

class AwsJson10Generator(
    symbolProvider: SymbolProvider,
    writer: RustWriter,
    private val operationShape: OperationShape,
    inputShape: StructureShape
) : HttpProtocolGenerator(symbolProvider, writer, inputShape) {
    override fun toHttpRequestImpl(implBlockWriter: RustWriter) {
        implBlockWriter.rustBlock("pub fn build_http_request(&self) -> \$T", RuntimeType.HttpRequestBuilder) {
            write("let builder = \$T::new();", RuntimeType.HttpRequestBuilder)
            write(
                """builder
|           .method("POST")
|           .header("Content-Type", "application/x-amz-json-1.0")
|           .header("X-Amz-Target", "JsonRpc10.${operationShape.id.name}")
|           """.trimMargin()
            )
        }
    }
}
