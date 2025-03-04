/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.traits.IncompatibleWithStalledStreamProtectionTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.util.logging.Logger

/**
 * This class provides model/shape transformers that disable stalled stream protection due to inherent incompatibility with certain operations.
 */
object DisableStalledStreamProtection {
    private val logger: Logger = Logger.getLogger(javaClass.name)

    /**
     * Removes stalled stream protection from a model when it meets any of the following conditions:
     *
     * * The operations support event streams
     */
    fun transformModel(model: Model): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(shape is OperationShape && shape.isEventStream(model)) {
                logger.info("$it is an event stream operation, adding IncompatibleWithStalledStreamProtection trait")
                (shape as OperationShape).toBuilder().addTrait(IncompatibleWithStalledStreamProtectionTrait()).build()
            }
        }

    /**
     * Removes stalled stream protection from the given [operation].
     *
     * Use this method when removal criteria are specific to the service operation and cannot be generalized for use with [transformModel],
     * e.g., S3's CopyObject operation.
     */
    fun transformOperation(operation: OperationShape): OperationShape {
        logger.info("Adding IncompatibleWithStalledStreamProtection trait to $operation")
        return operation.toBuilder().addTrait(IncompatibleWithStalledStreamProtectionTrait()).build()
    }
}
