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

internal class EventStreamMessageTimeoutTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service EventService {
            operations: [PublishEvents, Echo]
        }

        @http(uri: "/publish", method: "POST")
        operation PublishEvents {
            input := {
                @httpPayload
                @required
                events: Events
            }
            output := {}
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

        @streaming
        union Events {
            event: Event
        }

        structure Event {
            data: String
        }
        """.asSmithyModel()

    private val streamingBlobModel =
        """
        ${'$'}version: "2.0"
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service StreamingService {
            operations: [StreamingUpload]
        }

        @http(uri: "/upload", method: "POST")
        operation StreamingUpload {
            input := {
                @httpPayload
                @required
                data: StreamingBlob
            }
        }

        @streaming
        blob StreamingBlob
        """.asSmithyModel()

    @Test
    fun `event stream message timeout is disabled by default`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                null,
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
        // Event stream operations still get no whole-body read deadline
        check(config.timeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
        // The message timeout never applies to non-event-stream operations
        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#Echo")) == null)
    }

    @Test
    fun `event stream message timeout default can be configured`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                objectNode(
                    """
                    {
                        "requestBodyReadTimeouts": {
                            "defaultEventStreamMessage": "30s"
                        }
                    }
                    """,
                ),
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == 30_000L)
        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#Echo")) == null)
        // The event stream message timeout doesn't enable the whole-body deadline for streaming operations
        check(config.timeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
    }

    @Test
    fun `per operation timeout on event stream operation configures the message timeout`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                objectNode(
                    """
                    {
                        "requestBodyReadTimeouts": {
                            "defaultEventStreamMessage": "30s",
                            "perOperation": {
                                "test#PublishEvents": "5s"
                            }
                        }
                    }
                    """,
                ),
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == 5_000L)
    }

    @Test
    fun `per operation zero disables the event stream message timeout`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#EventService"),
                objectNode(
                    """
                    {
                        "requestBodyReadTimeouts": {
                            "defaultEventStreamMessage": "30s",
                            "perOperation": {
                                "test#PublishEvents": 0
                            }
                        }
                    }
                    """,
                ),
            )

        check(config.eventStreamMessageTimeoutMillisFor(ShapeId.from("test#PublishEvents")) == null)
    }

    @Test
    fun `per operation timeout on streaming blob operation is still rejected`() {
        val error =
            assertThrows<CodegenException> {
                RequestBodyReadTimeouts.fromCustomizationConfig(
                    streamingBlobModel,
                    ShapeId.from("test#StreamingService"),
                    objectNode(
                        """
                        {
                            "requestBodyReadTimeouts": {
                                "perOperation": {
                                    "test#StreamingUpload": "30s"
                                }
                            }
                        }
                        """,
                    ),
                )
            }

        check(error.message?.contains("are not supported for streaming inputs") == true)
    }

    @Test
    fun `service compiles with event stream message timeout and generated code applies it`() {
        val generatedServers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = messageTimeoutSettings()),
            )
        generatedServers.forEach { generatedServer ->
            val protocolSerde = protocolSerdeContents(generatedServer.path)
            check(protocolSerde.contains("wrap_with_message_timeout(")) {
                "expected the generated deserializer to apply the event stream message timeout"
            }
            check(protocolSerde.contains("::std::time::Duration::from_millis(45000u64)")) {
                "expected the configured 45s timeout in the generated deserializer"
            }
        }
    }

    @Test
    fun `generated code does not apply a message timeout when not configured`() {
        val generatedServers = serverIntegrationTest(model)
        generatedServers.forEach { generatedServer ->
            val protocolSerde = protocolSerdeContents(generatedServer.path)
            check(!protocolSerde.contains("wrap_with_message_timeout(")) {
                "the event stream message timeout must be off by default"
            }
        }
    }

    private fun protocolSerdeContents(crateDir: java.nio.file.Path): String =
        crateDir.resolve("src/protocol_serde").toFile().walkTopDown()
            .filter { it.isFile && it.extension == "rs" }
            .joinToString("\n") { it.readText() }

    private fun messageTimeoutSettings(): ObjectNode =
        objectNode(
            """
            {
                "customizationConfig": {
                    "requestBodyReadTimeouts": {
                        "defaultEventStreamMessage": "45s"
                    }
                }
            }
            """,
        )

    private fun objectNode(json: String): ObjectNode = Node.parse(json).expectObjectNode()
}
