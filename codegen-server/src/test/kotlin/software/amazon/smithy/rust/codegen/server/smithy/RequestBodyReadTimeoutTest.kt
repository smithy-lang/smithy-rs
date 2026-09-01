/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class RequestBodyReadTimeoutTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            operations: [Echo, Health]
        }

        @http(uri: "/echo", method: "POST")
        operation Echo {
            input := {
                @required
                message: String
            }
            output := {
                @required
                message: String
            }
        }

        @http(uri: "/health", method: "GET")
        operation Health {}
        """.asSmithyModel()

    @Test
    fun `service compiles with request read timeout customization`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = readTimeoutSettings()),
        )
    }

    @Test
    fun `service compiles with default request read timeout`() {
        serverIntegrationTest(model)
    }

    @Test
    fun `service compiles with disabled request read timeout`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = disabledReadTimeoutSettings()),
        )
    }

    @Test
    fun `default request read timeout is one minute`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                null,
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == 60_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 60_000L)
    }

    @Test
    fun `default request read timeout can be disabled`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "readTimeouts": {
                        "defaultMillis": 0
                    }
                }
                """,
            )

        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                customizationConfig,
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == null)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == null)
    }

    @Test
    fun `operation request read timeout can be disabled`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "readTimeouts": {
                        "defaultMillis": 60000,
                        "operationMillis": {
                            "test#Echo": 0
                        }
                    }
                }
                """,
            )

        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                customizationConfig,
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == null)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 60_000L)
    }

    @Test
    fun `operation request read timeout can override disabled default`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "readTimeouts": {
                        "defaultMillis": 0,
                        "operationMillis": {
                            "test#Echo": 300000
                        }
                    }
                }
                """,
            )

        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                customizationConfig,
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == 300_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == null)
    }

    @Test
    fun `operation override falls back to default request read timeout`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "readTimeouts": {
                        "operationMillis": {
                            "test#Echo": 300000
                        }
                    }
                }
                """,
            )

        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                customizationConfig,
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == 300_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 60_000L)
    }

    @Test
    fun `no body operation override is accepted`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                readTimeoutSettings().expectObjectMember("customizationConfig"),
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 30_000L)
    }

    @Test
    fun `invalid operation override is rejected`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "readTimeouts": {
                        "defaultMillis": 10000,
                        "operationMillis": {
                            "test#Missing": 30000
                        }
                    }
                }
                """,
            )

        assertThrows<CodegenException> {
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                customizationConfig,
            )
        }
    }

    private fun readTimeoutSettings(): ObjectNode =
        objectNode(
            """
            {
                "customizationConfig": {
                    "readTimeouts": {
                        "defaultMillis": 10000,
                        "operationMillis": {
                            "test#Echo": 300000,
                            "test#Health": 30000
                        }
                    }
                }
            }
            """,
        )

    private fun disabledReadTimeoutSettings(): ObjectNode =
        objectNode(
            """
            {
                "customizationConfig": {
                    "readTimeouts": {
                        "defaultMillis": 0
                    }
                }
            }
            """,
        )

    private fun objectNode(json: String): ObjectNode =
        Node.parse(json).expectObjectNode()
}
