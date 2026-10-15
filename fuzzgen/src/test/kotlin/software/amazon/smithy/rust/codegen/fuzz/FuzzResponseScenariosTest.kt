/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.fuzz

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class FuzzResponseScenariosTest {
    private fun model(secondValue: String) =
        """
        ${'$'}version: "2"
        namespace test.scenarioaliases
        @aws.protocols#restJson1
        @aws.protocols#awsJson1_1
        service Scenarios { version: "1", operations: [Choose] }
        @http(method: "POST", uri: "/choose")
        @smithy.test#httpResponseTests([
            { id: "Rest", tags: ["fuzz:default", "fuzz:populated"], protocol: "aws.protocols#restJson1",
              code: 200, params: { value: "first" } },
            { id: "Json", tags: ["fuzz:default", "fuzz:populated"], protocol: "aws.protocols#awsJson1_1",
              code: 200, params: { value: "$secondValue" } }
        ])
        operation Choose { input: Input, output: Output }
        structure Input { scenario: String }
        structure Output { value: String }
        """.asSmithyModel()

    @Test
    fun `aliases retain identical typed values across protocol fixtures`() {
        val model = model("first")
        val operation = model.expectShape(ShapeId.from("test.scenarioaliases#Choose"), OperationShape::class.java)
        val cases = fuzzResponseScenarios(model, operation)
        assertEquals(listOf("default", "populated"), cases.map { it.name })
        assertEquals(cases.first().test.params, cases.last().test.params)
    }

    @Test
    fun `different values across protocols cannot silently select the first fixture`() {
        val model = model("different")
        val operation = model.expectShape(ShapeId.from("test.scenarioaliases#Choose"), OperationShape::class.java)
        assertThrows(IllegalArgumentException::class.java) { fuzzResponseScenarios(model, operation) }
    }
}
