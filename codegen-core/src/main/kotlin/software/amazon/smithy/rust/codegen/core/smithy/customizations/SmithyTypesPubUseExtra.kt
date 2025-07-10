/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customizations

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.eventReceiver
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamOperations
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember

/** Returns true if the model has normal streaming operations (excluding event streams) */
private fun hasStreamingOperations(
    model: Model,
    serviceShape: ServiceShape,
): Boolean {
    val operations = TopDownIndex.of(model).getContainedOperations(serviceShape)
    return operations.any { operation ->
        val input = model.expectShape(operation.inputShape, StructureShape::class.java)
        val output = model.expectShape(operation.outputShape, StructureShape::class.java)
        (input.hasStreamingMember(model) && !input.hasEventStreamMember(model)) ||
            (output.hasStreamingMember(model) && !output.hasEventStreamMember(model))
    }
}

// TODO(https://github.com/smithy-lang/smithy-rs/issues/2111): Fix this logic to consider collection/map shapes
private fun structUnionMembersMatchPredicate(
    model: Model,
    predicate: (Shape) -> Boolean,
): Boolean =
    model.structureShapes.any { structure ->
        structure.members().any { member -> predicate(model.expectShape(member.target)) }
    } ||
        model.unionShapes.any { union ->
            union.members().any { member -> predicate(model.expectShape(member.target)) }
        }

/** Returns true if the model uses any blob shapes */
private fun hasBlobs(model: Model): Boolean = structUnionMembersMatchPredicate(model, Shape::isBlobShape)

/** Returns true if the model uses any timestamp shapes */
private fun hasDateTimes(model: Model): Boolean = structUnionMembersMatchPredicate(model, Shape::isTimestampShape)

/** Adds re-export statements for Smithy primitives */
fun pubUseSmithyPrimitives(
    codegenContext: CodegenContext,
    model: Model,
    rustCrate: RustCrate,
): Writable =
    writable {
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
        if (hasStreamingOperations(model, codegenContext.serviceShape)) {
            rustCrate.mergeFeature(
                Feature(
                    "rt-tokio",
                    true,
                    listOf("aws-smithy-types/rt-tokio", "aws-smithy-types/http-body-1-x"),
                ),
            )
            rustTemplate(
                """
                pub use #{ByteStream};
                pub use #{AggregatedBytes};
                pub use #{Error} as ByteStreamError;
                ##[cfg(feature = "rt-tokio")]
                pub use #{FsBuilder};
                ##[cfg(feature = "rt-tokio")]
                pub use #{Length};
                pub use #{SdkBody};
                """,
                "ByteStream" to RuntimeType.smithyTypes(rc).resolve("byte_stream::ByteStream"),
                "AggregatedBytes" to RuntimeType.smithyTypes(rc).resolve("byte_stream::AggregatedBytes"),
                "Error" to RuntimeType.smithyTypes(rc).resolve("byte_stream::error::Error"),
                "FsBuilder" to RuntimeType.smithyTypes(rc).resolve("byte_stream::FsBuilder"),
                "Length" to RuntimeType.smithyTypes(rc).resolve("byte_stream::Length"),
                "SdkBody" to RuntimeType.smithyTypes(rc).resolve("body::SdkBody"),
            )
        }
    }

/** Adds re-export statements for event-stream-related Smithy primitives */
fun pubUseSmithyPrimitivesEventStream(
    codegenContext: CodegenContext,
    model: Model,
): Writable =
    writable {
        val rc = codegenContext.runtimeConfig
        if (codegenContext.serviceShape.hasEventStreamOperations(model)) {
            rustTemplate(
                """
                pub use #{EventReceiver};
                pub use #{Header};
                pub use #{HeaderValue};
                pub use #{Message};
                pub use #{StrBytes};
                """,
                "EventReceiver" to eventReceiver(rc),
                "Header" to RuntimeType.smithyTypes(rc).resolve("event_stream::Header"),
                "HeaderValue" to RuntimeType.smithyTypes(rc).resolve("event_stream::HeaderValue"),
                "Message" to RuntimeType.smithyTypes(rc).resolve("event_stream::Message"),
                "StrBytes" to RuntimeType.smithyTypes(rc).resolve("str_bytes::StrBytes"),
            )
        }
    }
