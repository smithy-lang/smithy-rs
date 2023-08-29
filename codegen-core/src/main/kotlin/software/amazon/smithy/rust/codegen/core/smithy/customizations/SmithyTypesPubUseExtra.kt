/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember

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

/** Adds re-export statements for Smithy primitives */
fun pubUseSmithyPrimitives(codegenContext: CodegenContext, model: Model): Writable = writable {
    val rc = codegenContext.runtimeConfig
    if (hasBlobs(model)) {
        rustTemplate("pub use #{Blob};", "Blob" to RuntimeType.blob(rc))
    }
    if (hasDateTimes(model)) {
        rustTemplate(
            """
            pub use #{DateTime};
            pub use #{Format} as DateTimeFormat;
            """,
            "DateTime" to RuntimeType.dateTime(rc),
            "Format" to RuntimeType.format(rc),
        )
    }
    if (hasStreamingOperations(model)) {
        rustTemplate(
            """
            pub use #{ByteStream};
            pub use #{AggregatedBytes};
            pub use #{Error} as ByteStreamError;
            pub use #{SdkBody};
            """,
            "ByteStream" to RuntimeType.smithyHttp(rc).resolve("byte_stream::ByteStream"),
            "AggregatedBytes" to RuntimeType.smithyHttp(rc).resolve("byte_stream::AggregatedBytes"),
            "Error" to RuntimeType.smithyHttp(rc).resolve("byte_stream::error::Error"),
            "SdkBody" to RuntimeType.smithyHttp(rc).resolve("body::SdkBody"),
        )
    }
}
