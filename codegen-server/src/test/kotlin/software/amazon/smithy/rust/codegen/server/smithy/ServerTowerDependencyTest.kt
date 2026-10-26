/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import com.moandjiezana.toml.Toml
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Generates servers for HTTP 0.x and 1.x with schemaSerde omitted, disabled, or enabled.
 * It compiles and runs Tower middleware that forwards a request and checks the successful response.
 * All generated Cargo.toml files must select Tower 0.5 with the util feature.
 * HTTP 1.x servers also exercise the metrics layer and plugin in both serde modes.
 */
internal class ServerTowerDependencyTest {
    @Test
    fun `HTTP 0x middleware builds with schema serde omitted`() = checkTowerMiddleware(false, null)

    @Test
    fun `HTTP 0x middleware builds with schema serde disabled`() = checkTowerMiddleware(false, false)

    @Test
    fun `HTTP 1x middleware builds with schema serde omitted`() = checkTowerMiddleware(true, null)

    @Test
    fun `HTTP 1x middleware builds with schema serde disabled`() = checkTowerMiddleware(true, false)

    @Test
    fun `HTTP 1x schema middleware builds with Tower 05`() = checkTowerMiddleware(true, true)

    private fun checkTowerMiddleware(
        http1x: Boolean,
        schemaSerde: Boolean?,
    ) {
        val model =
            """
            ${'$'}version: "2"
            namespace test
            use aws.protocols#restJson1
            use smithy.test#httpRequestTests

            @restJson1
            service Example {
                version: "1"
                operations: [Ping]
            }

            @http(method: "POST", uri: "/ping")
            @httpRequestTests([{
                id: "PingRequest"
                protocol: "aws.protocols#restJson1"
                method: "POST"
                uri: "/ping"
                headers: {"Content-Type": "application/json"}
                params: {}
                appliesTo: "server"
            }])
            operation Ping {
                input := {}
                output := {}
            }
            """.asSmithyModel()

        val settings = ServerAdditionalSettings.builder().withHttp1x(http1x)
        if (schemaSerde != null) {
            settings.schemaSerde(schemaSerde)
        }
        val server =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = settings.toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, rustCrate ->
                // Existing decorators can still use the original public property.
                val middlewareTower =
                    if (context.usesSchemaHttpSerde) ServerCargoDependency.tower(context) else ServerCargoDependency.Tower
                val scope =
                    arrayOf(
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to middlewareTower.toType(),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    )
                rustCrate.testModule {
                    tokioTest("tower_layer_and_service_ext_route_a_request") {
                        rustTemplate(
                            """
                            let config = crate::ExampleConfig::builder()
                                .layer(#{Tower}::layer::util::Identity::new())
                                .build();
                            let app = crate::Example::builder(config)
                                .ping(|_input: crate::input::PingInput| async { crate::output::PingOutput {} })
                                .build().expect("service should build");
                            let request = #{Http}::Request::builder()
                                .method("POST")
                                .uri("/ping")
                                .header("Content-Type", "application/json")
                                .body(#{Server}::body::Body::empty()).unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                            assert_eq!(response.status().as_u16(), 200);
                            assert_eq!(response.headers()["content-type"], "application/json");
                            assert_eq!(response.headers()["content-length"], "2");
                            #{ReadResponse}
                            assert_eq!(bytes.as_ref(), b"{}");

                            {
                                let request = #{Http}::Request::builder()
                                    .method("POST")
                                    .uri("/missing")
                                    .header("Content-Type", "application/json")
                                    .body(#{Server}::body::Body::empty()).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                                // Observed with mainline codegen and runtime before upgrading Tower.
                                assert_eq!(response.status().as_u16(), 404);
                                assert_eq!(response.headers()["content-type"], "application/json");
                                assert_eq!(response.headers()["x-amzn-errortype"], "UnknownOperationException");
                                #{ReadResponse}
                                assert_eq!(bytes.as_ref(), b"{}");
                            }
                            """,
                            *scope,
                            "ReadResponse" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "bytes", "response"),
                        )
                    }
                    if (http1x) {
                        tokioTest("metrics_middleware_routes_a_request") {
                            rustTemplate(
                                """
                                let config = crate::ExampleConfig::builder()
                                    .http_plugin(#{Server}::plugin::HttpPlugins::new()
                                        .push(#{Metrics}::plugin::DefaultMetricsPlugin))
                                    .build();
                                let app = crate::Example::builder(config)
                                    .ping(|_input: crate::input::PingInput| async { crate::output::PingOutput {} })
                                    .build().expect("service should build");
                                let sink = #{Metrique}::test_util::test_entry_sink();
                                let app = #{Tower}::Layer::layer(
                                    &#{Metrics}::MetricsLayer::new_with_sink(sink.sink.clone()), app);
                                // MetricsLayer accepts Hyper's Incoming body. Use an in-memory
                                // HTTP connection to exercise the same body type as a real server.
                                let (client_io, server_io) = #{Tokio}::io::duplex(4096);
                                let server = #{Tokio}::spawn(async move {
                                    #{Hyper}::server::conn::http1::Builder::new()
                                        .serve_connection(
                                            #{HyperUtil}::rt::TokioIo::new(server_io),
                                            #{HyperUtil}::service::TowerToHyperService::new(app),
                                        ).await.unwrap();
                                });
                                let (mut client, connection) = #{Hyper}::client::conn::http1::handshake(
                                    #{HyperUtil}::rt::TokioIo::new(client_io)).await.unwrap();
                                let connection = #{Tokio}::spawn(connection);
                                let request = #{Http}::Request::builder()
                                    .method("POST")
                                    .uri("/ping")
                                    .header("Content-Type", "application/json")
                                    .body(#{Server}::body::Body::empty()).unwrap();
                                let response = client.send_request(request).await.unwrap();
                                assert_eq!(response.status().as_u16(), 200);
                                assert_eq!(response.headers()["content-type"], "application/json");
                                #{ReadResponse}
                                assert_eq!(bytes.as_ref(), b"{}");
                                drop(client);
                                connection.await.unwrap().unwrap();
                                server.await.unwrap();
                                assert_eq!(sink.inspector.entries().len(), 1);
                                """,
                                *scope,
                                "Metrics" to context.runtimeConfig.smithyRuntimeCrate("smithy-http-server-metrics").toType(),
                                "Metrique" to CargoDependency("metrique", CratesIo("0.1.33")).toDevDependency().withFeature("test-util").toType(),
                                "Tokio" to CargoDependency.Tokio.withFeature("io-util").toDevDependency().toType(),
                                "Hyper" to ServerCargoDependency.hyperDev(context.runtimeConfig).withFeature("client").withFeature("http1").toType(),
                                "HyperUtil" to CargoDependency("hyper-util", CratesIo("0.1"), features = setOf("tokio", "service")).toDevDependency().toType(),
                                "ReadResponse" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "bytes", "response"),
                            )
                        }
                    }
                }
            }.single()
        val tower = Toml().read(server.path.resolve("Cargo.toml").toFile()).getTable("dependencies.tower")
        tower.getString("version") shouldBe "0.5"
        tower.getList<String>("features") shouldBe listOf("util")
    }
}
