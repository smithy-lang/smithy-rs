/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.transformers.DisableStalledStreamProtection
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.util.logging.Logger

/**
 * Disables stalled stream protection for specific operations.
 *
 * While a generic client-level decorator, `DisableStalledStreamProtection`, exists to handle this
 * at the model level, certain cases require operation-specific removal criteria that cannot be
 * generalized. (If we can fully generate the criteria, this class can be removed.)
 *
 * This class serves as a centralized solution for disabling stalled stream protection in such cases,
 * preventing the need for service-specific decorators solely for this purpose.
 */
class AwsDisableStalledStreamProtection : ClientCodegenDecorator {
    // These long-running operations may have times with no data transfer,
    // violating stalled stream protection.
    private val operationsIncompatibleWithStalledStreamProtection =
        setOf(
            ShapeId.from("com.amazonaws.lambda#Invoke"),
            ShapeId.from("com.amazonaws.lambda#InvokeAsync"),
            ShapeId.from("com.amazonaws.s3#CopyObject"),
        )

    override val name: String = "AwsDisableStalledStreamProtection"
    override val order: Byte = 0
    private val logger = Logger.getLogger(javaClass.name)

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(operationsIncompatibleWithStalledStreamProtection.contains(shape.id)) {
                logger.info("Adding IncompatibleWithStalledStreamProtection trait to $it")
                (DisableStalledStreamProtection::transformOperation)((it as OperationShape))
            }
        }
}
