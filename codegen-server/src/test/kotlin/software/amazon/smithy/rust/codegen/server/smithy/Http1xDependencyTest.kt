/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests for HTTP dependency selection based on http-1x flag.
 * Verifies that the correct dependencies are included in Cargo.toml and that code compiles.
 */
internal class Http1xDependencyTest {
    private val testModel =
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

    private fun buildAdditionalSettings(http1x: Boolean) =
        Node.objectNodeBuilder()
            .withMember(
                "codegen",
                Node.objectNodeBuilder()
                    .withMember("http-1x", http1x)
                    .build(),
            ).build()

    @Test
    fun `SDK with http-1x enabled compiles and has correct dependencies`() {
        val testDir =
            serverIntegrationTest(
                testModel,
                IntegrationTestParams(
                    additionalSettings = buildAdditionalSettings(http1x = true),
                ),
            )

        val cargoToml = testDir.resolve("Cargo.toml").toFile()
        assertTrue(cargoToml.exists(), "Cargo.toml should exist")

        val cargoTomlContent = cargoToml.readText()

        // Should have HTTP 1.x dependencies
        assertTrue(
            cargoTomlContent.contains(Regex("""\[dependencies\.http\]\s+version\s*=\s*"1"""", RegexOption.MULTILINE)),
            "Should have http version 1.x",
        )
        assertTrue(
            cargoTomlContent.contains("http-body-util"),
            "Should have http-body-util dependency",
        )
        assertTrue(
            cargoTomlContent.contains("aws-smithy-http-server"),
            "Should have aws-smithy-http-server dependency",
        )

        // Should NOT have legacy dependencies
        assertFalse(
            cargoTomlContent.contains("aws-smithy-http-legacy-server"),
            "Should NOT have aws-smithy-http-legacy-server dependency",
        )
    }

    @Test
    fun `SDK with http-1x disabled compiles and has correct dependencies`() {
        val testDir =
            serverIntegrationTest(
                testModel,
                IntegrationTestParams(
                    additionalSettings = buildAdditionalSettings(http1x = false),
                ),
            )

        val cargoToml = testDir.resolve("Cargo.toml").toFile()
        assertTrue(cargoToml.exists(), "Cargo.toml should exist")

        val cargoTomlContent = cargoToml.readText()

        // Should have HTTP 0.x dependencies
        assertTrue(
            cargoTomlContent.contains(Regex("""\[dependencies\.http\]\s+version\s*=\s*"0\.2""", RegexOption.MULTILINE)),
            "Should have http version 0.2.x",
        )
        assertTrue(
            cargoTomlContent.contains("aws-smithy-http-legacy-server"),
            "Should have aws-smithy-http-legacy-server dependency",
        )

        // Should NOT have HTTP 1.x specific dependencies
        assertFalse(
            cargoTomlContent.contains("http-body-util"),
            "Should NOT have http-body-util dependency for HTTP 0.x",
        )
        assertFalse(
            cargoTomlContent.contains(Regex("""\[dependencies\.aws-smithy-http-server\].*"http-1x"""", setOf(RegexOption.MULTILINE, RegexOption.DOT_MATCHES_ALL))),
            "Should NOT have aws-smithy-http-server with http-1x feature",
        )

        // Verify pinned dependencies from HttpDependencies
        assertTrue(
            cargoTomlContent.contains(Regex("""\[dependencies\.aws-smithy-json\]\s+version\s*=\s*"0\.61"""", RegexOption.MULTILINE)),
            "Should have pinned aws-smithy-json version 0.61.x",
        )
        assertTrue(
            cargoTomlContent.contains(Regex("""\[dependencies\.aws-smithy-http\]\s+version\s*=\s*"0\.62"""", RegexOption.MULTILINE)),
            "Should have pinned aws-smithy-http version 0.62.x",
        )
    }

    @Test
    fun `SDK defaults to http-0x when no flag is provided`() {
        val testDir =
            serverIntegrationTest(
                testModel,
                IntegrationTestParams(),
            )

        val cargoToml = testDir.resolve("Cargo.toml").toFile()
        assertTrue(cargoToml.exists(), "Cargo.toml should exist")

        val cargoTomlContent = cargoToml.readText()

        // Should default to HTTP 0.x
        assertTrue(
            cargoTomlContent.contains("aws-smithy-http-legacy-server"),
            "Should default to aws-smithy-http-legacy-server dependency",
        )
        assertFalse(
            cargoTomlContent.contains("http-body-util"),
            "Should NOT have http-body-util dependency by default",
        )
    }
}
