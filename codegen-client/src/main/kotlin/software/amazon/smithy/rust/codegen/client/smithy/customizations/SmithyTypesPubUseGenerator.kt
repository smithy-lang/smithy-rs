/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember

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
            PubUseType(RuntimeType.blob(runtimeConfig), ::hasBlobs),
            PubUseType(RuntimeType.dateTime(runtimeConfig), ::hasDateTimes),
        ) + RuntimeType.smithyTypes(runtimeConfig).let { types ->
            listOf(PubUseType(types.resolve("error::display::DisplayErrorContext")) { true })
        } + RuntimeType.smithyHttp(runtimeConfig).let { http ->
            listOf(
                PubUseType(http.resolve("result::SdkError")) { true },
                PubUseType(http.resolve("byte_stream::ByteStream"), ::hasStreamingOperations),
                PubUseType(http.resolve("byte_stream::AggregatedBytes"), ::hasStreamingOperations),
            )
        }
        ).filter { pubUseType -> pubUseType.shouldExport(model) }.map { it.type }
}

/** Adds re-export statements in a separate file for the types module */
fun pubUseSmithyTypes(runtimeConfig: RuntimeConfig, model: Model, rustCrate: RustCrate) {
    rustCrate.withModule(RustModule.Types) {
        val types = pubUseTypes(runtimeConfig, model)
        if (types.isNotEmpty()) {
            types.forEach { type -> rust("pub use #T;", type) }
        }
    }
}
