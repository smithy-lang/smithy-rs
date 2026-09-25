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
            operations: [Echo, Health, Upload]
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

        @http(uri: "/upload", method: "POST")
        operation Upload {
            input := {
                @httpPayload
                data: Blob
            }
            output := {}
        }
        """.asSmithyModel()

    private val streamingModel =
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
    fun `globally disabled request body read timeout allows a slow body`() {
        assertDisabledTimeoutAllowsSlowBody(disabledReadTimeoutSettings())
    }

    @Test
    fun `operation disabled request body read timeout allows a slow body`() {
        assertDisabledTimeoutAllowsSlowBody(operationDisabledReadTimeoutSettings())
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

                    async fn slow_echo(
                        input: crate::input::EchoInput,
                    ) -> Result<crate::output::EchoOutput, crate::error::EchoError> {
                        #{Tokio}::time::sleep(std::time::Duration::from_millis(300)).await;
                        Ok(crate::output::EchoOutput {
                            message: input.message,
                        })
                    }

                    async fn upload(
                        _input: crate::input::UploadInput,
                    ) -> crate::output::UploadOutput {
                        crate::output::UploadOutput {}
                    }
                    """,
                    "Tokio" to RuntimeType.Tokio,
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

                tokioTest("slow_http_payload_returns_request_timeout_over_http") {
                    rustTemplate(
                        """
                        use #{Tokio}::io::{AsyncReadExt, AsyncWriteExt};

                        let config = crate::TestServiceConfig::builder().build();
                        let app = crate::TestService::builder(config)
                            .upload(upload)
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
                                b"POST /upload HTTP/1.1\r\n\
                                  Host: localhost\r\n\
                                  Content-Type: application/octet-stream\r\n\
                                  Content-Length: 100\r\n\
                                  \r\n\
                                  partial payload",
                            )
                            .await
                            .expect("failed to write partial payload");

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
                            "unexpected payload response: {response:?}",
                        );
                        assert!(
                            response.to_ascii_lowercase().contains("\r\nconnection: close\r\n"),
                            "payload response missing connection close: {response:?}",
                        );
                        """,
                        "StartServer" to startServer,
                        "Tokio" to RuntimeType.Tokio,
                    )
                }

                tokioTest("slow_handler_is_not_subject_to_request_body_read_timeout") {
                    rustTemplate(
                        """
                        use #{Tokio}::io::{AsyncReadExt, AsyncWriteExt};

                        let config = crate::TestServiceConfig::builder().build();
                        let app = crate::TestService::builder(config)
                            .echo(slow_echo)
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
                                  Content-Length: 19\r\n\
                                  Connection: close\r\n\
                                  \r\n\
                                  {\"message\":\"hello\"}",
                            )
                            .await
                            .expect("failed to write request");

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
                            response.starts_with("HTTP/1.1 200 OK"),
                            "handler was incorrectly subject to the request body read timeout: {response:?}",
                        );
                        """,
                        "StartServer" to startServer,
                        "Tokio" to RuntimeType.Tokio,
                    )
                }
            }
        }
    }

    private fun assertDisabledTimeoutAllowsSlowBody(settings: ObjectNode) {
        serverIntegrationTest(
            model,
            IntegrationTestParams(additionalSettings = settings),
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

                tokioTest("disabled_request_body_read_timeout_allows_a_slow_body") {
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
                        // Use raw TCP so the request body can be paused midway through transmission.
                        stream
                            .write_all(
                                b"POST /echo HTTP/1.1\r\n\
                                  Host: localhost\r\n\
                                  Content-Type: application/json\r\n\
                                  Content-Length: 19\r\n\
                                  Connection: close\r\n\
                                  \r\n\
                                  {\"message\"",
                            )
                            .await
                            .expect("failed to write partial request");

                        #{Tokio}::time::sleep(std::time::Duration::from_millis(300)).await;

                        stream
                            .write_all(b":\"hello\"}")
                            .await
                            .expect("failed to finish request");

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
                            response.starts_with("HTTP/1.1 200 OK"),
                            "disabled request body read timeout rejected a slow body: {response:?}",
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
    fun `streaming operations do not receive default request body read timeouts`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                streamingModel,
                ShapeId.from("test#StreamingService"),
                null,
            )

        check(config.timeoutMillisFor(ShapeId.from("test#StreamingUpload")) == null)
    }

    @Test
    fun `explicit request body read timeout for streaming operation is rejected`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "perOperation": {
                            "test#StreamingUpload": "30m"
                        }
                    }
                }
                """,
            )

        val error =
            assertThrows<CodegenException> {
                RequestBodyReadTimeouts.fromCustomizationConfig(
                    streamingModel,
                    ShapeId.from("test#StreamingService"),
                    customizationConfig,
                )
            }

        check(error.message?.contains("are not supported for streaming inputs") == true)
    }

    @Test
    fun `default request read timeout uses payload aware defaults`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                null,
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == 60_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 60_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == 36_000_000L)
    }

    @Test
    fun `payload and non payload request read timeout defaults can be configured separately`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "15s",
                        "defaultPayload": "5m"
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

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == 15_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 15_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == 300_000L)
    }

    @Test
    fun `request read timeout values accept string units`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "10 s",
                        "defaultPayload": "1h",
                        "perOperation": {
                            "test#Echo": "300000ms",
                            "test#Health": "5m",
                            "test#Upload": "120s"
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
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 300_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == 120_000L)
    }

    @Test
    fun `payload and non payload request read timeout defaults can be disabled separately`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": 0,
                        "defaultPayload": 0
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
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == null)
    }

    @Test
    fun `operation request read timeout can be disabled`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "1m",
                        "defaultPayload": "1m",
                        "perOperation": {
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
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == 60_000L)
    }

    @Test
    fun `operation request read timeout can override disabled default`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": 0,
                        "defaultPayload": 0,
                        "perOperation": {
                            "test#Echo": "5m"
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
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == null)
    }

    @Test
    fun `operation override falls back to payload aware default request read timeouts`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "perOperation": {
                            "test#Echo": "5m"
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
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == 36_000_000L)
    }

    @Test
    fun `operation overrides win over payload aware defaults`() {
        val config =
            RequestBodyReadTimeouts.fromCustomizationConfig(
                model,
                ShapeId.from("test#TestService"),
                readTimeoutSettings().expectObjectMember("customizationConfig"),
            )

        check(config.timeoutMillisFor(ShapeId.from("test#Echo")) == 300_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Health")) == 30_000L)
        check(config.timeoutMillisFor(ShapeId.from("test#Upload")) == 120_000L)
    }

    @Test
    fun `positive numeric request read timeout is rejected`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": 3000
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

    @Test
    fun `unitless string request read timeout is rejected`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "3000"
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

    @Test
    fun `invalid request read timeout unit is rejected`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "3d"
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

    @Test
    fun `invalid operation override is rejected`() {
        val customizationConfig =
            objectNode(
                """
                {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "10s",
                        "defaultPayload": "1m",
                        "perOperation": {
                            "test#Missing": "30s"
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
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "10s",
                        "defaultPayload": "1m",
                        "perOperation": {
                            "test#Echo": "5m",
                            "test#Health": "30s",
                            "test#Upload": "2m"
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
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": 0,
                        "defaultPayload": 0
                    }
                }
            }
            """,
        )

    private fun operationDisabledReadTimeoutSettings(): ObjectNode =
        objectNode(
            """
            {
                "customizationConfig": {
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "100ms",
                        "defaultPayload": "100ms",
                        "perOperation": {
                            "test#Echo": 0
                        }
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
                    "requestBodyReadTimeouts": {
                        "defaultNonPayload": "2s",
                        "defaultPayload": "100ms",
                        "perOperation": {
                            "test#Echo": "100ms"
                        }
                    }
                }
            }
            """,
        )

    private fun objectNode(json: String): ObjectNode = Node.parse(json).expectObjectNode()
}
