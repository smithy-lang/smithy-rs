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
            IntegrationTestParams(command = {}), // Skip cargo compilation
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
                command = {}, // Skip cargo compilation
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
                command = {}, // Skip cargo compilation
            ),
        ) { codegenContext, _ ->
            assertFalse(codegenContext.settings.codegenConfig.http1x)
        }
    }

    @Test
    fun `httpDependencies returns correct dependencies when http1x is disabled`() {
        serverIntegrationTest(
            baseModel,
            IntegrationTestParams(command = {}), // Skip cargo compilation
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
                command = {}, // Skip cargo compilation
            ),
        ) { codegenContext, _ ->
            val httpDeps = codegenContext.httpDependencies()

            assertEquals("http-1x", httpDeps.http.name)
            assertEquals("http-body-1x", httpDeps.httpBody.name)
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
            IntegrationTestParams(command = {}), // Skip cargo compilation
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
}
