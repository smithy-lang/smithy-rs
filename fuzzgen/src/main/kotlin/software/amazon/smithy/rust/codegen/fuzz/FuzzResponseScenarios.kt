/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.protocoltests.traits.HttpResponseTestCase
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.core.util.outputShape

/** Explicit opt-in: protocol fixtures tagged `fuzz:<scenario>` supply handler return values. */
internal data class FuzzResponseScenario(
    val name: String,
    val shape: StructureShape,
    val test: HttpResponseTestCase,
    val error: Boolean,
)

internal fun fuzzResponseScenarios(
    model: Model,
    operation: OperationShape,
): List<FuzzResponseScenario> {
    val cases = mutableListOf<FuzzResponseScenario>()
    val targets =
        listOf(operation.outputShape(model) to false) +
            operation.errors.map { model.expectShape(it, StructureShape::class.java) to true }
    for ((shape, error) in targets) {
        val owner = if (error) shape else operation
        for (test in owner.getTrait(HttpResponseTestsTrait::class.java).orElse(null)?.testCases.orEmpty()) {
            val tags = test.tags.filter { it.startsWith("fuzz:") }
            for (tag in tags) {
                val scenario = FuzzResponseScenario(tag.removePrefix("fuzz:"), shape, test, error)
                require(scenario.name.isNotEmpty()) { "response fixture ${test.id} declares an empty fuzz scenario" }
                val existing = cases.find { it.name == scenario.name }
                if (existing != null) {
                    // A multi-protocol service repeats expected wire fixtures. Handler values must
                    // remain identical across protocols; don't choose whichever fixture happens first.
                    require(existing.shape.id == shape.id && existing.test.params == test.params && existing.error == error) {
                        "fuzz scenario ${scenario.name} on ${operation.id} has different typed values across protocol fixtures"
                    }
                } else {
                    cases.add(scenario)
                }
            }
        }
    }
    return cases
}
