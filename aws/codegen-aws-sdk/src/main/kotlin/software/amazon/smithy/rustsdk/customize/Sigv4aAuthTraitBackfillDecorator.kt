/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.aws.traits.auth.SigV4ATrait
import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.AuthTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.sdkId
import software.amazon.smithy.rustsdk.SigV4AuthDecorator

class Sigv4aAuthTraitBackfillDecorator : ClientCodegenDecorator {
    private val needsSigv4aBackfill =
        setOf(
            "CloudFront KeyValueStore",
            "EventBridge",
            "S3",
            "SESv2",
        )

    override val name: String get() = "Sigv4aAuthTraitBackfill"

    // This decorator must decorate before SigV4AuthDecorator so the model transformer of this class runs first
    override val order: Byte = (SigV4AuthDecorator.ORDER + 1).toByte()

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model {
        if (!applies(service)) {
            return model
        }

        return ModelTransformer.create().mapShapes(model) { shape ->
            when (shape.isServiceShape) {
                true -> {
                    val builder = (shape as ServiceShape).toBuilder()

                    if (!shape.hasTrait<SigV4ATrait>()) {
                        builder.addTrait(
                            SigV4ATrait.builder()
                                .name(
                                    shape.getTrait<SigV4Trait>()?.name
                                        ?: shape.getTrait<ServiceTrait>()?.arnNamespace,
                                )
                                .build(),
                        )
                    }

                    // SigV4A is appended at the end because these services implement SigV4A
                    // through endpoint-specific rules rather than the service shape.
                    // To ensure correct prioritization, it's safest to add it last,
                    // letting the endpoint rules take precedence as needed.
                    val authTrait =
                        shape.getTrait<AuthTrait>()?.let {
                            if (it.valueSet.contains(SigV4ATrait.ID)) {
                                it
                            } else {
                                AuthTrait(it.valueSet + mutableSetOf(SigV4ATrait.ID))
                            }
                        } ?: AuthTrait(mutableSetOf(SigV4Trait.ID, SigV4ATrait.ID))
                    builder.addTrait(authTrait)

                    builder.build()
                }

                false -> {
                    shape
                }
            }
        }
    }

    private fun applies(service: ServiceShape) = needsSigv4aBackfill.contains(service.sdkId())
}
