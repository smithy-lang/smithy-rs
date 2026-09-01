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
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
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
    fun `slow request body returns request timeout over http`() {
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = wireReadTimeoutSettings()),
        ) { codegenContext, rustCrate ->
            val startServer =
                writable {
                    when (codegenContext.runtimeConfig.httpVersion) {
                        HttpVersion.Http0x ->
                            rustTemplate(
                                """
                                let std_listener = listener.into_std().expect("failed to convert listener");
                                let server = #{Tokio}::spawn(async move {
                                    #{Hyper}::Server::from_tcp(std_listener)
                                        .expect("failed to create server")
                                        .serve(app.into_make_service())
                                        .await
                                        .expect("server failed");
                                });
                                """,
                                "Hyper" to ServerCargoDependency.hyperDev(codegenContext.runtimeConfig).toType(),
                                "Tokio" to RuntimeType.Tokio,
                            )

                        HttpVersion.Http1x ->
                            rustTemplate(
                                """
                                let server = #{Tokio}::spawn(async move {
                                    crate::serve(listener, app.into_make_service())
                                        .configure_hyper(|builder| builder.http1_only())
                                        .await
                                        .expect("server failed");
                                });
                                """,
                                "Tokio" to RuntimeType.Tokio,
                            )
                    }
                }

            rustCrate.testModule {
                rustTemplate(
                    """
                    async fn echo(
                        input: crate::input::EchoInput,
                    ) -> Result<crate::output::EchoOutput, crate::error::EchoError> {
                        Ok(crate::output::EchoOutput {
                            message: input.message,
                        })
                    }
                    """,
                )

                tokioTest("slow_request_body_returns_request_timeout_over_http") {
                    rustTemplate(
                        """
                        use #{Tokio}::io::{AsyncReadExt, AsyncWriteExt};

                        let config = crate::TestServiceConfig::builder().build();
                        let app = crate::TestService::builder(config)
                            .echo(echo)
                            .build_unchecked();

                        let listener = #{Tokio}::net::TcpListener::bind("127.0.0.1:0")
                            .await
                            .expect("failed to bind listener");
                        let addr = listener.local_addr().expect("failed to get local address");
                        #{StartServer:W}

                        let mut stream = #{Tokio}::net::TcpStream::connect(addr)
                            .await
                            .expect("failed to connect to server");
                        stream
                            .write_all(
                                b"POST /echo HTTP/1.1\r\n\
                                  Host: localhost\r\n\
                                  Content-Type: application/json\r\n\
                                  Content-Length: 100\r\n\
                                  \r\n\
                                  {\"message\"",
                            )
                            .await
                            .expect("failed to write partial request");

                        let mut response = Vec::new();
                        #{Tokio}::time::timeout(
                            std::time::Duration::from_secs(2),
                            stream.read_to_end(&mut response),
                        )
                        .await
                        .expect("timed out waiting for response")
                        .expect("failed to read response");
                        server.abort();

                        let response = String::from_utf8_lossy(&response);
                        assert!(
                            response.starts_with("HTTP/1.1 408 Request Timeout"),
                            "unexpected response: {response:?}",
                        );
                        assert!(
                            response.to_ascii_lowercase().contains("\r\nconnection: close\r\n"),
                            "response missing connection close: {response:?}",
                        );
                        """,
                        "StartServer" to startServer,
                        "Tokio" to RuntimeType.Tokio,
                    )
                }
            }
        }
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

    private fun wireReadTimeoutSettings(): ObjectNode =
        objectNode(
            """
            {
                "customizationConfig": {
                    "readTimeouts": {
                        "defaultMillis": 100
                    }
                }
            }
            """,
        )

    private fun objectNode(json: String): ObjectNode =
        Node.parse(json).expectObjectNode()
}
