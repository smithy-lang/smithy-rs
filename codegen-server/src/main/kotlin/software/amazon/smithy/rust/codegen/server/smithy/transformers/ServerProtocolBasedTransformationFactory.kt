package software.amazon.smithy.rust.codegen.server.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.AbstractShapeBuilder
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.utils.SmithyBuilder
import software.amazon.smithy.utils.ToSmithyBuilder

/**
 * Each protocol may not support all of the features that Smithy allows. For instance, most
 * RPC protocols do not support HTTP bindings. `ServerProtocolBasedTransformationFactory` is a factory
 * object that transforms the model and removes specific traits based on the protocol being instantiated.
 */
object ServerProtocolBasedTransformationFactory {
    fun createTransformer(protocolShapeId: ShapeId): Transformer =
        when (protocolShapeId) {
            Rpcv2CborTrait.ID -> Rpcv2Transformer()
            else -> IdentityTransformer()
        }

    interface Transformer {
        fun transform(
            model: Model,
            service: ServiceShape,
        ): Model
    }

    fun <T : Shape, B> T.removeTraitIfPresent(
        traitId: ShapeId,
    ): T
        where T : ToSmithyBuilder<T>,
              B : AbstractShapeBuilder<B, T>,
              B : SmithyBuilder<T> {
        return if (this.hasTrait(traitId)) {
            @Suppress("UNCHECKED_CAST")
            (this.toBuilder() as B).removeTrait(traitId).build()
        } else {
            this
        }
    }

    class Rpcv2Transformer() : Transformer {
        override fun transform(
            model: Model,
            service: ServiceShape,
        ): Model {
            val transformedModel =
                ModelTransformer.create().mapShapes(model) { shape ->
                    when (shape) {
                        is OperationShape -> shape.removeTraitIfPresent(HttpTrait.ID)
                        is MemberShape -> {
                            shape
                                .removeTraitIfPresent(HttpLabelTrait.ID)
                                .removeTraitIfPresent(HttpPayloadTrait.ID)
                        }

                        else -> shape
                    }
                }

            return transformedModel
        }
    }

    class IdentityTransformer() : Transformer {
        override fun transform(
            model: Model,
            service: ServiceShape,
        ): Model {
            return model
        }
    }
}
