/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.util.findStreamingMember
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.logging.Logger

/** Transformer to REMOVE operations that use EventStreaming until event streaming is supported */
object RemoveEventStreamOperations {
    private val logger = Logger.getLogger(javaClass.name)

    private fun eventStreamEnabled(): Boolean =
        System.getenv()["SMITHYRS_EXPERIMENTAL_EVENTSTREAM"] == "1"

    fun transform(model: Model): Model {
        if (eventStreamEnabled()) {
            return model
        }
        return ModelTransformer.create().filterShapes(model) { parentShape ->
            if (parentShape !is OperationShape) {
                true
            } else {
                val ioShapes = listOfNotNull(parentShape.output.orNull(), parentShape.input.orNull()).map {
                    model.expectShape(
                        it,
                        StructureShape::class.java
                    )
                }
                val hasEventStream = ioShapes.any { ioShape ->
                    val streamingMember = ioShape.findStreamingMember(model)?.let { model.expectShape(it.target) }
                    streamingMember?.isUnionShape ?: false
                }
                // If a streaming member has a union trait, it is an event stream. Event Streams are not currently supported
                // by the SDK, so if we generate this API it won't work.
                (!hasEventStream).also {
                    if (!it) {
                        logger.info("Removed $parentShape from model because it targets an event stream")
                    }
                }
            }
        }
    }
}
