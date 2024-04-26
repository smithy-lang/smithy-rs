/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.KnowledgeIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId

val traitsUsed = mutableMapOf<ShapeId, Set<ShapeId>>()

class AtLeastOneServiceOperationUsesTraitIndex(private val model: Model) : KnowledgeIndex {
    init {
        for (service in model.serviceShapes) {
            // We only need to load traits for a given service once.
            if (traitsUsed.containsKey(service.id)) {
                continue
            } else {
                val usedTraits = mutableSetOf<ShapeId>()
                for (operation in service.operations.map { model.expectShape(it) as OperationShape }) {
                    usedTraits.addAll(operation.allTraits.values.map { it.toShapeId() })
                }
                traitsUsed[service.id] = usedTraits
            }
        }
    }

    fun usesTrait(traitShapeId: ShapeId): Boolean {
        return model.serviceShapes.map { it.id }.any {
            // Non-null assertion is safe because `init` will populate the map when creating this index.
            return traitsUsed[it]!!.any { it == traitShapeId }
        }
    }

    companion object {
        fun of(model: Model): AtLeastOneServiceOperationUsesTraitIndex {
            return model.getKnowledge(AtLeastOneServiceOperationUsesTraitIndex::class.java) {
                AtLeastOneServiceOperationUsesTraitIndex(it)
            }
        }
    }
}
