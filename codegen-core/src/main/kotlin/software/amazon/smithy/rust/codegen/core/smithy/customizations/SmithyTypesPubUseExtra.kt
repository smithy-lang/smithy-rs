/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.core.util.letIf

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

// TODO(https://github.com/awslabs/smithy-rs/issues/2111): Fix this logic to consider collection/map shapes
private fun structUnionMembersMatchPredicate(model: Model, predicate: (Shape) -> Boolean): Boolean =
    model.structureShapes.any { structure ->
        structure.members().any { member -> predicate(model.expectShape(member.target)) }
    } || model.unionShapes.any { union ->
        union.members().any { member -> predicate(model.expectShape(member.target)) }
    }

/** Returns true if the model uses any blob shapes */
private fun hasBlobs(model: Model): Boolean = structUnionMembersMatchPredicate(model, Shape::isBlobShape)

/** Returns true if the model uses any timestamp shapes */
private fun hasDateTimes(model: Model): Boolean = structUnionMembersMatchPredicate(model, Shape::isTimestampShape)

/** Returns a list of types that should be re-exported for the given model */
internal fun pubUseTypes(codegenContext: CodegenContext, model: Model): List<RuntimeType> {
    val runtimeConfig = codegenContext.runtimeConfig
    return (
        listOf(
            PubUseType(RuntimeType.blob(runtimeConfig), ::hasBlobs),
            PubUseType(RuntimeType.dateTime(runtimeConfig), ::hasDateTimes),
        ) + RuntimeType.smithyHttp(runtimeConfig).let { http ->
            listOf(
                PubUseType(http.resolve("byte_stream::ByteStream"), ::hasStreamingOperations),
                PubUseType(http.resolve("byte_stream::AggregatedBytes"), ::hasStreamingOperations),
            )
        }
        ).filter { pubUseType -> pubUseType.shouldExport(model) }.map { it.type }
}

/** Adds re-export statements for Smithy primitives */
fun pubUseSmithyPrimitives(codegenContext: CodegenContext, model: Model): Writable = writable {
    val types = pubUseTypes(codegenContext, model)
    if (types.isNotEmpty()) {
        types.forEach { type -> rust("pub use #T;", type) }
    }
}

/** Adds re-export statements for error types */
fun pubUseSmithyErrorTypes(codegenContext: CodegenContext): Writable = writable {
    val runtimeConfig = codegenContext.runtimeConfig
    val reexports = listOf(
        listOf(
            RuntimeType.smithyHttp(runtimeConfig).let { http ->
                PubUseType(http.resolve("result::SdkError")) { true }
            },
        ),
        RuntimeType.smithyTypes(runtimeConfig).let { types ->
            listOf(PubUseType(types.resolve("error::display::DisplayErrorContext")) { true })
                // Only re-export `ProvideErrorMetadata` for clients
                .letIf(codegenContext.target == CodegenTarget.CLIENT) { list ->
                    list + listOf(PubUseType(types.resolve("error::metadata::ProvideErrorMetadata")) { true })
                }
        },
    ).flatten()
    reexports.forEach { reexport ->
        rust("pub use #T;", reexport.type)
    }
}
