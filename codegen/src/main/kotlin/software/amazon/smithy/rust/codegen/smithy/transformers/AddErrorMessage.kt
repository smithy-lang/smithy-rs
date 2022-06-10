/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.orNull
import java.util.logging.Logger

fun StructureShape.errorMessageMember(): MemberShape? = this.getMember("message").or { this.getMember("Message") }.orNull()

/**
 * Ensure that all errors have error messages.
 *
 * Not all errors are modeled with an error message field. However, in many cases, the server can still send an error.
 * If an error, specifically, a structure shape with the error trait does not have a member `message` or `Message`,
 * this transformer will add a `message` member targeting a string.
 *
 * This ensures that we always generate a modeled error message field enabling end users to easily extract the error
 * message when present.
 *
 * Currently, this is run on all models, however, we may restrict this to AWS SDK code generation in the future.
 */
object AddErrorMessage {
    private val logger = Logger.getLogger("AddErrorMessage")

    /**
     * Ensure that all errors have error messages.
     */
    fun transform(model: Model): Model {
        return ModelTransformer.create().mapShapes(model) { shape ->
            val addMessageField = shape.hasTrait<ErrorTrait>() && shape is StructureShape && shape.errorMessageMember() == null
            if (addMessageField && shape is StructureShape) {
                logger.info("Adding message field to ${shape.id}")
                shape.toBuilder().addMember("message", ShapeId.from("smithy.api#String")).build()
            } else {
                shape
            }
        }
    }
}
