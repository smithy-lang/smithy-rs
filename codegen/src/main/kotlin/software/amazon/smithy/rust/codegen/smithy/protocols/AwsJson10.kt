/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.RustType
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.lang.stripOuter
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.Serializers
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.HttpProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.locatedIn
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.traits.InputBodyTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer

sealed class AwsJsonVersion {
    abstract val value: String

    object Json10 : AwsJsonVersion() {
        override val value = "1.0"
    }

    object Json11 : AwsJsonVersion() {
        override val value = "1.1"
    }
}

class BasicAwsJsonFactory(private val version: AwsJsonVersion) : ProtocolGeneratorFactory<BasicAwsJsonGenerator> {
    override fun buildProtocolGenerator(
        protocolConfig: ProtocolConfig
    ): BasicAwsJsonGenerator = BasicAwsJsonGenerator(protocolConfig, version)

    override fun transformModel(model: Model): Model {
        // For AwsJson10, the body matches 1:1 with the input
        return OperationNormalizer().transformModel(model) { inputShape -> inputShape }
    }

    override fun symbolProvider(model: Model, base: SymbolProvider): SymbolProvider {
        return SyntheticBodySymbolProvider(model, base)
    }
}

/**
 * SyntheticBodySymbolProvider makes two modifications:
 * 1. Body shapes are moved to `serializer.rs`
 * 2. Body shapes take a reference to all of their members.
 */
class SyntheticBodySymbolProvider(private val model: Model, private val base: SymbolProvider) :
    WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val initialSymbol = base.toSymbol(shape)
        val override = when (shape) {
            is StructureShape -> if (shape.hasTrait(InputBodyTrait::class.java)) {
                initialSymbol.toBuilder().locatedIn(Serializers).build()
            } else null
            is MemberShape -> {
                val container = model.expectShape(shape.container)
                if (container.hasTrait(InputBodyTrait::class.java)) {
                    initialSymbol.toBuilder().rustType(
                        RustType.Reference(
                            lifetime = "a",
                            value = initialSymbol.rustType().stripOuter<RustType.Box>()
                        )
                    ).build()
                } else {
                    null
                }
            }
            else -> null
        }
        return override ?: initialSymbol
    }
}

class BasicAwsJsonGenerator(
    private val protocolConfig: ProtocolConfig,
    private val awsJsonVersion: AwsJsonVersion
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
                   .header("Content-Type", "application/x-amz-json-${awsJsonVersion.value}")
                   .header("X-Amz-Target", "${protocolConfig.serviceShape.id.name}.${operationShape.id.name}")
               """.trimMargin()
            )
        }
    }
}
