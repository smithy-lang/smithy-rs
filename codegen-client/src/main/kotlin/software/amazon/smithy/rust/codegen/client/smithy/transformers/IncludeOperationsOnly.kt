/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.transform.ModelTransformer
import java.util.logging.Logger

/**
 * Filters a Smithy model to include only specified operations.
 * This reduces generated code size and compilation time for large services.
 */
object IncludeOperationsOnly {
    private val logger: Logger = Logger.getLogger(javaClass.name)

    /**
     * Transforms the model to include only the specified operations.
     *
     * @param model The original Smithy model
     * @param operationsToInclude Set of operation names to keep (e.g., ["PutObject", "GetObject"])
     * @return Transformed model with filtered operations, or original model if operationsToInclude is empty
     * @throws IllegalArgumentException if any specified operation doesn't exist in the service
     */
    fun transformModel(
        model: Model,
        operationsToInclude: Set<String>,
    ): Model {
        if (operationsToInclude.isEmpty()) {
            return model
        }

        val filteredModel =
            ModelTransformer.create().mapShapes(model) { shape ->
                if (shape is ServiceShape) {
                    val allOperationNames = shape.allOperations.map { it.name }.toSet()

                    // Validate that all requested operations exist
                    val invalidOperations = operationsToInclude - allOperationNames
                    if (invalidOperations.isNotEmpty()) {
                        throw IllegalArgumentException(
                            "Operations not found in service ${shape.id}: ${invalidOperations.joinToString(", ")}\n" +
                                "Available operations: ${allOperationNames.sorted().joinToString(", ")}",
                        )
                    }

                    val filteredOperations =
                        shape.allOperations
                            .filter { operationsToInclude.contains(it.name) }
                            .toSet()

                    logger.info(
                        "Filtering service ${shape.id}: keeping ${filteredOperations.size}/${allOperationNames.size} operations",
                    )

                    shape.toBuilder()
                        .operations(filteredOperations)
                        .build()
                } else {
                    shape
                }
            }

        // Remove unreferenced shapes to reduce generated code
        return ModelTransformer.create().removeUnreferencedShapes(filteredModel)
    }
}
