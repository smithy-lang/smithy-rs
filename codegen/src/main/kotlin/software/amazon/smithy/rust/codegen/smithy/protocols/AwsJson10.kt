/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory

class AwsJson10Factory : ProtocolGeneratorFactory<AwsJson10Generator> {
    override fun buildProtocolGenerator(
        protocolConfig: ProtocolConfig
    ): AwsJson10Generator = AwsJson10Generator(protocolConfig)
}

class AwsJson10Generator(
    private val protocolConfig: ProtocolConfig
) : HttpProtocolGenerator(protocolConfig) {
    override fun toHttpRequestImpl(
        implBlockWriter: RustWriter,
        operationShape: OperationShape,
        inputShape: StructureShape
    ) {
        implBlockWriter.rustBlock("pub fn build_http_request(&self) -> \$T", RuntimeType.HttpRequestBuilder) {
            write("let builder = \$T::new();", RuntimeType.HttpRequestBuilder)
            write(
                """
                builder
                   .method("POST")
                   .header("Content-Type", "application/x-amz-json-1.0")
                   .header("X-Amz-Target", "${protocolConfig.serviceShape.id.name}.${operationShape.id.name}")
               """.trimMargin()
            )
        }
    }
}
