/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.api.assertThrows
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import kotlin.time.Duration
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.Duration.Companion.seconds

internal class HeaderReadTimeoutTest {
    private val model =
        """
        ${'$'}version: "2.0"
        namespace test
        use aws.protocols#restJson1
        @restJson1
        service HeaderService { operations: [Health] }
        @http(uri: "/health", method: "GET")
        operation Health { input := {}, output := {} }
        """.asSmithyModel()

    private fun settings(value: String?): ServerRustSettings =
        ServerRustSettings.from(
            model,
            Node.parse(
                """
                {
                    "service":"test#HeaderService", "module":"header-service",
                    "moduleVersion":"0.0.1", "moduleAuthors":["test"],
                    "customizationConfig":${customization(value)}
                }
                """,
            ).expectObjectNode(),
        )

    private fun customization(value: String?): String =
        """{"protocols":{"common":{"request":{"header":{${value?.let { "\"readTimeout\":$it" } ?: ""}}}}}}"""

    @Test
    fun `header timeout uses the normalized common section and existing duration parser`() {
        check(settings(null).headerReadTimeout == null)
        for ((value, expected) in listOf(
            "0" to Duration.ZERO,
            "\"0ms\"" to Duration.ZERO,
            "\"0s\"" to Duration.ZERO,
            "\"0m\"" to Duration.ZERO,
            "\"0h\"" to Duration.ZERO,
            "\"250ms\"" to 250.milliseconds,
            "\" 10s \"" to 10.seconds,
            "\"2m\"" to 120.seconds,
            "\"1h\"" to 3600.seconds,
        )) {
            check(settings(value).headerReadTimeout == expected)
        }
        val copied =
            settings("\"10s\"").copy(
                customizationConfig = Node.parse(customization("0")).expectObjectNode(),
            )
        check(copied.headerReadTimeout == Duration.ZERO)
        val protocolOnly =
            settings(null).copy(
                customizationConfig =
                    Node.parse("""{"protocols":{"aws.protocols#restJson1":{"request":{"header":{"readTimeout":"1s"}}}}}""").expectObjectNode(),
            )
        val error = assertThrows<CodegenException> { protocolOnly.headerReadTimeout }
        check(error.message!!.contains("customizationConfig.protocols.aws.protocols#restJson1"))
    }

    @ParameterizedTest
    @ValueSource(strings = ["1", "-1", "0.5", "true", "null", "{}", "[]", "\"10\"", "\"-1s\"", "\"1.5s\"", "\"10d\"", "\"\"", "\"9223372036854775808s\"", "\"9223372036854775807h\""])
    fun `invalid header timeout fails eagerly with its full configuration path`(value: String) {
        val error = assertThrows<CodegenException> { settings(value) }
        check(error.message!!.contains("customizationConfig.protocols.common.request.header.readTimeout"))
    }

    @ParameterizedTest
    @ValueSource(strings = ["omitted", "\"100ms\"", "0", "\"0s\""])
    fun `generated configuration and serve apply the setting and allow runtime overrides`(value: String) {
        val setting = value.takeUnless { it == "omitted" }
        val expected =
            when (value) {
                "omitted" -> "#{Some}(Duration::from_secs(60))"
                "\"100ms\"" -> "#{Some}(Duration::from_millis(100))"
                else -> "#{None}"
            }
        val expires = value == "\"100ms\""
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = Node.parse("""{"customizationConfig":${customization(setting)}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
        ) { context, rustCrate ->
            rustCrate.testModule {
                tokioTest("typed_header_configuration") {
                    rustTemplate(
                        """
                        use std::time::Duration;
                        let config = crate::service_config::service_config();
                        assert_eq!(config.header_read_timeout, $expected);
                        let defaults = #{Server}::config::ServiceConfig::default();
                        assert_eq!(defaults.header_read_timeout, #{Some}(Duration::from_secs(60)));
                        assert_eq!(defaults.clone().with_header_read_timeout(#{Some}(Duration::ZERO)).header_read_timeout, #{None});
                        assert_eq!(defaults.clone().with_header_read_timeout(#{None}).header_read_timeout, #{None});
                        assert_eq!(defaults.with_header_read_timeout(#{Some}(Duration::from_secs(1))).header_read_timeout, #{Some}(Duration::from_secs(1)));
                        """,
                        *RuntimeType.preludeScope,
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    )
                }
                if (context.runtimeConfig.httpVersion == HttpVersion.Http1x) {
                    tokioTest("generated_serve_header_deadlines_over_tcp") {
                        rustTemplate(
                            """
                            use std::time::Duration;
                            use #{Tokio}::io::{AsyncReadExt, AsyncWriteExt};
                            use #{Tokio}::net::{TcpListener, TcpStream};
                            use #{Tokio}::time::{sleep, timeout};
                            use #{Server}::serve::Serve;

                            async fn health(_: crate::input::HealthInput) -> crate::output::HealthOutput {
                                crate::output::HealthOutput {}
                            }
                            // Exercise the unmodified generated call, and runtime overrides that
                            // enable, disable, or lengthen the generated deadline.
                            for (override_timeout, expires) in [
                                (#{None}, $expires),
                                (#{Some}(#{Some}(Duration::from_millis(50))), true),
                                (#{Some}(#{None}), false),
                                (#{Some}(#{Some}(Duration::ZERO)), false),
                                (#{Some}(#{Some}(Duration::from_secs(2))), false),
                            ] {
                                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                                let addr = listener.local_addr().unwrap();
                                let config = crate::HeaderServiceConfig::builder().build();
                                let app = crate::HeaderService::builder(config).health(health).build().unwrap();
                                // This annotation verifies that the public return type is unchanged.
                                let server: Serve<_, _, _, _> = crate::serve(listener, app.into_make_service());
                                let server = match override_timeout {
                                    #{Some}(value) => server.header_read_timeout(value),
                                    #{None} => server,
                                };
                                let (shutdown_tx, shutdown_rx) = #{Tokio}::sync::oneshot::channel();
                                let task = #{Tokio}::spawn(async move {
                                    server.with_graceful_shutdown(async { let _ = shutdown_rx.await; }).await.unwrap();
                                });
                                let mut stream = TcpStream::connect(addr).await.unwrap();
                                stream.write_all(b"GET /health HTTP/1.1\r\nHost: localhost\r\n").await.unwrap();
                                if expires {
                                    let mut response = #{Vec}::new();
                                    timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
                                        .await.expect("configured header deadline").unwrap();
                                    // Hyper can send 408 or simply close an incomplete head.
                                    assert!(response.is_empty() || response.starts_with(b"HTTP/1.1 408 "));
                                } else {
                                    // Wait past the short generated deadline, then complete the head.
                                    sleep(Duration::from_millis(250)).await;
                                    stream.write_all(b"Connection: close\r\n\r\n").await.unwrap();
                                    let mut response = #{Vec}::new();
                                    timeout(Duration::from_secs(2), stream.read_to_end(&mut response)).await.unwrap().unwrap();
                                    assert!(response.starts_with(b"HTTP/1.1 200 "), "{response:?}");
                                }
                                drop(stream);
                                shutdown_tx.send(()).unwrap();
                                timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
                            }
                            """,
                            *RuntimeType.preludeScope,
                            "Tokio" to CargoDependency.Tokio.withFeature("full").toDevDependency().toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        )
                    }
                }
            }
        }
    }
}
