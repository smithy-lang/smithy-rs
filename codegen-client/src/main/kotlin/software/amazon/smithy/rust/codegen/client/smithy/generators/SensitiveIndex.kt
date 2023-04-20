/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.KnowledgeIndex
import software.amazon.smithy.model.selector.Selector
import software.amazon.smithy.model.shapes.OperationShape

class SensitiveIndex(model: Model) : KnowledgeIndex {
    private val sensitiveInputSelector = Selector.parse("operation:test(-[input]-> ~> [trait|sensitive])")
    private val sensitiveOutputSelector = Selector.parse("operation:test(-[output]-> ~> [trait|sensitive])")
    private val sensitiveInputs = sensitiveInputSelector.select(model).map { it.id }.toSet()
    private val sensitiveOutputs = sensitiveOutputSelector.select(model).map { it.id }.toSet()

    fun hasSensitiveInput(operationShape: OperationShape): Boolean = sensitiveInputs.contains(operationShape.id)
    fun hasSensitiveOutput(operationShape: OperationShape): Boolean = sensitiveOutputs.contains(operationShape.id)

    companion object {
        fun of(model: Model): SensitiveIndex {
            return model.getKnowledge(SensitiveIndex::class.java) {
                SensitiveIndex(it)
            }
        }
    }
}
