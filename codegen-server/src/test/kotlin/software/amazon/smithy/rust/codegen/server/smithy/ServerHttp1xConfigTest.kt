/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests for the http-1x configuration flag.
 * Phase 1: Foundation - verifies that the configuration can be parsed correctly.
 */
internal class ServerHttp1xConfigTest {
    private val baseModel =
        """
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            version: "2024-03-18"
            operations: [GetStatus]
        }

        @http(uri: "/status", method: "GET")
        operation GetStatus {
            output: GetStatusOutput
        }

        structure GetStatusOutput {
            status: String
        }
        """.asSmithyModel()

    @Test
    fun `http1x defaults to false when not specified`() {
        serverIntegrationTest(
            baseModel,
            IntegrationTestParams(command = {}),
        ) { codegenContext, _ ->
            assertFalse(codegenContext.settings.codegenConfig.http1x)
        }
    }

    @Test
    fun `http1x can be enabled via configuration`() {
        serverIntegrationTest(
            baseModel,
            IntegrationTestParams(
                additionalSettings =
                    Node.objectNodeBuilder()
                        .withMember(
                            "codegen",
                            Node.objectNodeBuilder()
                                .withMember("http-1x", true)
                                .build(),
                        ).build(),
                command = {},
            ),
        ) { codegenContext, _ ->
            assertTrue(codegenContext.settings.codegenConfig.http1x)
        }
    }

    @Test
    fun `http1x can be disabled explicitly`() {
        serverIntegrationTest(
            baseModel,
            IntegrationTestParams(
                additionalSettings =
                    Node.objectNodeBuilder()
                        .withMember(
                            "codegen",
                            Node.objectNodeBuilder()
                                .withMember("http-1x", false)
                                .build(),
                        ).build(),
                command = {},
            ),
        ) { codegenContext, _ ->
            assertFalse(codegenContext.settings.codegenConfig.http1x)
        }
    }

    @Test
    fun `httpDependencies returns correct dependencies when http1x is disabled`() {
        serverIntegrationTest(
            baseModel,
            IntegrationTestParams(command = {}),
        ) { codegenContext, _ ->
            val httpDeps = codegenContext.httpDependencies()

            assertEquals("http", httpDeps.http.name)
            assertEquals("http-body", httpDeps.httpBody.name)
            assertNull(httpDeps.httpBodyUtil)
            assertTrue(httpDeps.smithyHttpServer.name.contains("smithy-http-server"))
            assertFalse(httpDeps.smithyHttpServer.features.contains("http-1x"))
        }
    }

    @Test
    fun `httpDependencies returns correct dependencies when http1x is enabled`() {
        serverIntegrationTest(
            baseModel,
            IntegrationTestParams(
                additionalSettings =
                    Node.objectNodeBuilder()
                        .withMember(
                            "codegen",
                            Node.objectNodeBuilder()
                                .withMember("http-1x", true)
                                .build(),
                        ).build(),
                command = {},
            ),
        ) { codegenContext, _ ->
            val httpDeps = codegenContext.httpDependencies()

            // Both modes use "http" as the dependency name, but with different versions
            assertEquals("http", httpDeps.http.name)
            assertEquals("http-body", httpDeps.httpBody.name)
            assertNotNull(httpDeps.httpBodyUtil)
            assertEquals("http-body-util", httpDeps.httpBodyUtil?.name)
            assertTrue(httpDeps.smithyHttpServer.name.contains("smithy-http-server"))
            assertTrue(httpDeps.smithyHttpServer.features.contains("http-1x"))
        }
    }

    @Test
    fun `httpDependencies helper methods return correct RuntimeTypes`() {
        serverIntegrationTest(
            baseModel,
            IntegrationTestParams(command = {}),
        ) { codegenContext, _ ->
            val httpDeps = codegenContext.httpDependencies()

            // Verify that helper methods work correctly
            assertEquals("http", httpDeps.httpModule().name)
            assertEquals("http", httpDeps.httpType().name)
            assertEquals("Request", httpDeps.httpRequest().name)
            assertEquals("Response", httpDeps.httpResponse().name)
            assertEquals("Method", httpDeps.httpMethod().name)
            assertEquals("HeaderMap", httpDeps.httpHeaderMap().name)
        }
    }

    @Test
    fun `generated Cargo toml has http 0x dependencies by default`() {
        val testDir =
            serverIntegrationTest(
                baseModel,
                IntegrationTestParams(command = {}),
            )

        val cargoToml = testDir.resolve("Cargo.toml").toFile()
        assertTrue(cargoToml.exists(), "Cargo.toml should exist")

        val cargoTomlContent = cargoToml.readText()

        // Should have [dependencies.http] with version = "0.2.x"
        assertTrue(
            cargoTomlContent.contains(Regex("""\[dependencies\.http\]\s+version\s*=\s*"0\.2""", RegexOption.MULTILINE)),
            "Cargo.toml should contain [dependencies.http] followed by version = \"0.2.x\"",
        )

        // Should NOT have http-body-util (only needed for HTTP 1.x)
        assertFalse(
            cargoTomlContent.contains("http-body-util"),
            "Cargo.toml should NOT contain http-body-util for HTTP 0.x",
        )

        // Should NOT have http version 1.x
        assertFalse(
            cargoTomlContent.contains(Regex("""\[dependencies\.http\]\s+version\s*=\s*"1"""", RegexOption.MULTILINE)),
            "Cargo.toml should NOT contain [dependencies.http] with version 1",
        )
    }

    @Test
    fun `generated Cargo toml has http 1x dependencies when enabled`() {
        val testDir =
            serverIntegrationTest(
                baseModel,
                IntegrationTestParams(
                    additionalSettings =
                        Node.objectNodeBuilder()
                            .withMember(
                                "codegen",
                                Node.objectNodeBuilder()
                                    .withMember("http-1x", true)
                                    .build(),
                            ).build(),
                    command = {},
                ),
            )

        val cargoToml = testDir.resolve("Cargo.toml").toFile()
        assertTrue(cargoToml.exists(), "Cargo.toml should exist")

        val cargoTomlContent = cargoToml.readText()

        // Debug: print content to see actual format
        println("=== Cargo.toml content (HTTP 1.x) ===")
        println(cargoTomlContent)
        println("=== End Cargo.toml ===")

        // Should have [dependencies.http] with version = "1"
        assertTrue(
            cargoTomlContent.contains(Regex("""\[dependencies\.http\]\s+version\s*=\s*"1"""", RegexOption.MULTILINE)),
            "Cargo.toml should contain [dependencies.http] with version = \"1\". Actual:\n$cargoTomlContent",
        )

        // Should have http-body-util
        assertTrue(
            cargoTomlContent.contains("http-body-util"),
            "Cargo.toml should contain http-body-util",
        )

        // Should have aws-smithy-http-server with http-1x feature
        assertTrue(
            cargoTomlContent.contains(Regex("""\[dependencies\.aws-smithy-http-server\].*features\s*=\s*\[.*"http-1x".*\]""", setOf(RegexOption.MULTILINE, RegexOption.DOT_MATCHES_ALL))),
            "Cargo.toml should have aws-smithy-http-server with http-1x feature",
        )

        // Should NOT have http 0.2.x
        assertFalse(
            cargoTomlContent.contains(Regex("""\[dependencies\.http\]\s+version\s*=\s*"0\.2""", RegexOption.MULTILINE)),
            "Cargo.toml should NOT contain [dependencies.http] with version 0.2.x",
        )
    }
}
