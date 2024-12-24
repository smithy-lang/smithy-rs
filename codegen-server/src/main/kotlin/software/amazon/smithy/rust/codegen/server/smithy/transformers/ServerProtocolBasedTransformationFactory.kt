/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.transformers

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.AbstractShapeBuilder
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpLabelTrait
import software.amazon.smithy.model.traits.HttpPayloadTrait
import software.amazon.smithy.model.traits.HttpTrait
import software.amazon.smithy.model.traits.StreamingTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.protocol.traits.Rpcv2CborTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.utils.SmithyBuilder
import software.amazon.smithy.utils.ToSmithyBuilder

/**
 * Each protocol may not support all of the features that Smithy allows. For instance, `rpcv2Cbor`
 * does not support HTTP bindings other than `@httpError`. `ServerProtocolBasedTransformationFactory` is a factory
 * object that transforms the model and removes specific traits based on the protocol being instantiated.
 *
 * In the long term, this class will be removed, and each protocol should be resilient enough to ignore extra
 * traits that the model is annotated with. This will be addressed when we fix issue
 * [#2979](https://github.com/smithy-lang/smithy-rs/issues/2979).
 */
object ServerProtocolBasedTransformationFactory {
    fun transform(
        model: Model,
        settings: ServerRustSettings,
    ): Model {
        val service = settings.getService(model)
        if (!service.hasTrait<Rpcv2CborTrait>()) {
            return model
        }

        // `rpcv2Cbor` does not support:
        // 1. `@httpPayload` trait.
        // 2. `@httpLabel` trait.
        // 3. `@streaming` trait applied to a `Blob` (data streaming).
        return ModelTransformer.create().mapShapes(model) { shape ->
            when (shape) {
                is OperationShape -> shape.removeTraitIfPresent(HttpTrait.ID)
                is MemberShape -> {
                    shape
                        .removeTraitIfPresent(HttpLabelTrait.ID)
                        .removeTraitIfPresent(HttpPayloadTrait.ID)
                }
                is BlobShape -> {
                    shape.removeTraitIfPresent(StreamingTrait.ID)
                }
                else -> shape
            }
        }
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
}
