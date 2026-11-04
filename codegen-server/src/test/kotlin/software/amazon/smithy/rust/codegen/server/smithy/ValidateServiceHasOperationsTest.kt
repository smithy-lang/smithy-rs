/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertDoesNotThrow
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext

class ValidateServiceHasOperationsTest {
    private val serviceId = ShapeId.from("test#Example")

    @Test
    fun `server code generation fails when the service has no operations`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service Example { version: "1" }
            """.asSmithyModel()
        val (context, _) = generatePluginContext(model, service = serviceId.toString())

        val exception = assertThrows<CodegenException> { RustServerCodegenPlugin().execute(context) }

        exception.message shouldContain "Service test#Example has no operations"
    }

    @Test
    fun `a service with only resource-bound operations passes validation`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service Example { version: "1", resources: [Thing] }

            resource Thing {
                identifiers: { thingId: String }
                read: GetThing
            }

            @readonly
            @http(uri: "/things/{thingId}", method: "GET")
            operation GetThing {
                input := {
                    @required
                    @httpLabel
                    thingId: String
                }
            }
            """.asSmithyModel()

        assertDoesNotThrow {
            validateServiceHasOperations(model, model.expectShape(serviceId, ServiceShape::class.java))
        }
    }

    @Test
    fun `a service with a direct operation passes validation`() {
        val model =
            """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service Example { version: "1", operations: [Ping] }

            @http(uri: "/ping", method: "GET")
            operation Ping {}
            """.asSmithyModel()

        assertDoesNotThrow {
            validateServiceHasOperations(model, model.expectShape(serviceId, ServiceShape::class.java))
        }
    }
}
