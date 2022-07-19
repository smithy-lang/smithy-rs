/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.util.hasStreamingMember

private data class PubUseType(
    val type: RuntimeType,
    val shouldExport: (Model) -> Boolean,
)

/** Returns true if the model has normal streaming operations (excluding event streams) */
private fun hasStreamingOperations(model: Model): Boolean {
    return model.operationShapes.any { operation ->
        val input = model.expectShape(operation.inputShape, StructureShape::class.java)
        val output = model.expectShape(operation.outputShape, StructureShape::class.java)
        (input.hasStreamingMember(model) && !input.hasEventStreamMember(model)) ||
            (output.hasStreamingMember(model) && !output.hasEventStreamMember(model))
    }
}

/** Returns true if the model has any blob shapes or members */
private fun hasBlobs(model: Model): Boolean {
    return model.structureShapes.any { structure ->
        structure.members().any { member -> model.expectShape(member.target).isBlobShape }
    } || model.unionShapes.any { union ->
        union.members().any { member -> model.expectShape(member.target).isBlobShape }
    }
}

/** Returns true if the model has any timestamp shapes or members */
private fun hasDateTimes(model: Model): Boolean {
    return model.structureShapes.any { structure ->
        structure.members().any { member -> model.expectShape(member.target).isTimestampShape }
    } || model.unionShapes.any { union ->
        union.members().any { member -> model.expectShape(member.target).isTimestampShape }
    }
}

/** Returns a list of types that should be re-exported for the given model */
internal fun pubUseTypes(runtimeConfig: RuntimeConfig, model: Model): List<RuntimeType> {
    return (
        listOf(
            PubUseType(RuntimeType.Blob(runtimeConfig), ::hasBlobs),
            PubUseType(RuntimeType.DateTime(runtimeConfig), ::hasDateTimes),
        ) + CargoDependency.SmithyHttp(runtimeConfig).asType().let { http ->
            listOf(
                PubUseType(http.member("result::SdkError")) { true },
                PubUseType(http.member("byte_stream::ByteStream"), ::hasStreamingOperations),
                PubUseType(http.member("byte_stream::AggregatedBytes"), ::hasStreamingOperations),
            )
        }
        ).filter { pubUseType -> pubUseType.shouldExport(model) }.map { it.type }
}

class SmithyTypesPubUseGenerator(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection) = writable {
        when (section) {
            is LibRsSection.Body -> {
                val types = pubUseTypes(runtimeConfig, section.model)
                if (types.isNotEmpty()) {
                    docs("Re-exported types from supporting crates.")
                    rustBlock("pub mod types") {
                        types.forEach { type -> rust("pub use #T;", type) }
                    }
                }
            }
            else -> {
            }
        }
    }
}
