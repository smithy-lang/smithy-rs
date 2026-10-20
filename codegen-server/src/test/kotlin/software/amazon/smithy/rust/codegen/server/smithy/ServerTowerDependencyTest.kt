/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import com.moandjiezana.toml.Toml
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

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

        var codegen = Node.objectNode().withMember("http-1x", http1x)
        if (schemaSerde != null) {
            codegen = codegen.withMember("schemaSerde", schemaSerde)
        }
        val server =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = Node.objectNode().withMember("codegen", codegen),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, rustCrate ->
                // Legacy decorators can still use the original public property.
                @Suppress("DEPRECATION")
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
                            let response = #{Tower}::ServiceExt::oneshot(app, request).await.unwrap();
                            assert_eq!(response.status().as_u16(), 200);
                            """,
                            *scope,
                        )
                    }
                }
            }.single()
        val tower = Toml().read(server.path.resolve("Cargo.toml").toFile()).getTable("dependencies.tower")
        tower.getString("version") shouldBe if (http1x && schemaSerde == true) "0.5" else "0.4"
        tower.getList<String>("features") shouldBe if (schemaSerde == true) listOf("util") else null
    }
}
