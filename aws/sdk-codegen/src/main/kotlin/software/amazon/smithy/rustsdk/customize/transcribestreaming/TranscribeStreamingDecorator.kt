/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.transcribestreaming

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.traits.IncompatibleWithStalledStreamProtectionTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.util.logging.Logger

/**
 * Top level decorator for TranscribeStreaming
 */
class TranscribeStreamingDecorator : ClientCodegenDecorator {
    private val operationsIncompatibleWithStalledStreamProtection =
        setOf(
            ShapeId.from("com.amazonaws.transcribestreaming#StartCallAnalyticsStreamTranscription"),
            ShapeId.from("com.amazonaws.transcribestreaming#StartMedicalStreamTranscription"),
            ShapeId.from("com.amazonaws.transcribestreaming#StartStreamTranscription"),
        )

    override val name: String = "TranscribeStreamingDecorator"
    override val order: Byte = 0
    private val logger = Logger.getLogger(javaClass.name)

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(shape.id in operationsIncompatibleWithStalledStreamProtection) {
                logger.info("Adding IncompatibleWithStalledStreamProtection trait to $it")
                (it as OperationShape).toBuilder().addTrait(IncompatibleWithStalledStreamProtectionTrait()).build()
            }
        }
}
