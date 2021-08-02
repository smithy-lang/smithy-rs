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

object AddErrorMessage {
    private val logger = Logger.getLogger("AddErrorMessage")
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
