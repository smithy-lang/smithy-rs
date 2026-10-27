/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.checkNoAdditionalHeaders
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Path

/**
 * Generates response protocol tests under legacy and schema serde and runs their Rust assertions
 * using the matching HTTP library types. Legacy cases cover HTTP 0.x and 1.x; schema cases cover
 * HTTP 1.x. Compilation and execution check that response-test helper dependencies and body
 * handling agree with the server's selected HTTP version.
 */
internal class ServerProtocolTestHelpersTest {
    private fun model(extraCases: String = "") =
        """
        ${'$'}version: "2"
        namespace test.protocolhelpers
        use aws.protocols#restJson1
        use smithy.test#httpResponseTests
        @restJson1
        service Helpers { version: "1", operations: [GetValue] }
        @http(method: "GET", uri: "/value")
        @httpResponseTests([{
            id: "ServerResponseAssertions",
            protocol: "aws.protocols#restJson1",
            code: 200,
            headers: { "content-type": "application/json", "x-value": "fixture" },
            requireHeaders: ["content-type", "x-value"],
            forbidHeaders: ["x-missing"],
            params: { header: "fixture", value: "body" },
            body: "{\"value\":\"body\"}",
            bodyMediaType: "application/json",
            appliesTo: "server"
        }, $extraCases])
        operation GetValue {
            input := {}
            output := { @httpHeader("x-value") header: String, value: String }
        }
        """.asSmithyModel()

    @ParameterizedTest
    @ValueSource(booleans = [false, true])
    fun `response assertions support the server HTTP version`(schemaSerde: Boolean) {
        val model =
            model(
                """
            {
                id: "OnlyDeclaredResponseHeaders",
                protocol: "aws.protocols#restJson1",
                code: 200,
                headers: { "Content-Type": "application/json", "X-Value": "fixture" },
                requireHeaders: ["Content-Length"],
                forbidHeaders: ["x-missing"],
                vendorParams: { forbidAdditionalHeaders: true },
                params: { header: "fixture", value: "body" },
                body: "{\"value\":\"body\"}",
                bodyMediaType: "application/json",
                appliesTo: "server"
            },
            {
                id: "AdditionalResponseHeadersAllowed",
                protocol: "aws.protocols#restJson1",
                code: 200,
                headers: { "content-type": "application/json" },
                vendorParams: { forbidAdditionalHeaders: false },
                params: { header: "fixture", value: "body" },
                appliesTo: "server"
            },
            {
                id: "ForbiddenHeaderStarIsLiteral",
                protocol: "aws.protocols#restJson1",
                code: 200,
                forbidHeaders: ["*"],
                params: { header: "fixture", value: "body" },
                appliesTo: "server"
            },
            {
                id: "UnexpectedResponseHeader",
                protocol: "aws.protocols#restJson1",
                code: 200,
                headers: { "content-type": "application/json" },
                requireHeaders: ["content-length"],
                vendorParams: { forbidAdditionalHeaders: true },
                params: { header: "fixture", value: "body" },
                appliesTo: "server"
            },
            {
                id: "EmptyResponseHeaderWhitelist",
                protocol: "aws.protocols#restJson1",
                code: 200,
                vendorParams: { forbidAdditionalHeaders: true },
                params: { header: "fixture", value: "body" },
                appliesTo: "server"
            }
            """,
            )
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                command = { path ->
                    val failures =
                        mapOf(
                            "unexpected_response_header_response" to "unexpected response header: x-value",
                            "empty_response_header_whitelist_response" to "unexpected response header:",
                        )
                    val (status, output) = cargoTest(path, failures.keys.flatMap { listOf("--skip", it) })
                    assertEquals(0, status, output)
                    // Execute negative fixtures separately so missing generated assertions fail CI.
                    for ((testName, diagnostic) in failures) {
                        val (failedStatus, failedOutput) = cargoTest(path, filter = testName)
                        assertEquals(101, failedStatus, failedOutput)
                        assertTrue(failedOutput.contains("1 failed"), failedOutput)
                        assertTrue(failedOutput.contains(diagnostic), failedOutput)
                    }
                },
            ),
            testCoverage =
                if (schemaSerde) HttpTestType.Only(HttpTestVersion.HTTP_1_X) else HttpTestType.ALL,
        ) { context, crate ->
            crate.testModule {
                unitTest("an_empty_whitelist_accepts_an_empty_header_map") {
                    rustTemplate(
                        "let headers = #{Http}::HeaderMap::new();",
                        "Http" to RuntimeType.http(context.runtimeConfig),
                    )
                    checkNoAdditionalHeaders(this, "headers", emptyList())
                }
            }
        }
    }

    @Test
    fun `the additional header option must be boolean`() {
        val exception =
            assertThrows(CodegenException::class.java) {
                serverIntegrationTest(
                    model(
                        """
                        {
                            id: "InvalidAdditionalHeaderOption",
                            protocol: "aws.protocols#restJson1",
                            code: 200,
                            vendorParams: { forbidAdditionalHeaders: "true" },
                            params: {},
                            appliesTo: "server"
                        }
                        """,
                    ),
                    IntegrationTestParams(command = {}),
                    testCoverage = HttpTestType.Default,
                )
            }
        assertTrue(exception.message!!.contains("InvalidAdditionalHeaderOption"), exception.message)
        assertTrue(exception.message!!.contains("vendorParams.forbidAdditionalHeaders must be a boolean"), exception.message)
    }

    private fun cargoTest(
        path: Path,
        testArguments: List<String> = emptyList(),
        filter: String? = null,
    ): Pair<Int, String> {
        val process =
            ProcessBuilder(
                listOf("cargo", "test", "--quiet", "--lib", "--tests") +
                    listOfNotNull(filter) + listOf("--") + testArguments,
            ).directory(path.toFile()).redirectErrorStream(true)
        process.environment()["RUSTFLAGS"] = "-D warnings"
        val running = process.start()
        val output = running.inputStream.bufferedReader().readText()
        return running.waitFor() to output
    }
}
