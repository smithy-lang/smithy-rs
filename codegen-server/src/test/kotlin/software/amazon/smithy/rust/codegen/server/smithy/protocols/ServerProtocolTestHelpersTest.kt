/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Generates response protocol tests under legacy and schema serde and runs their Rust assertions
 * using the matching HTTP library types. Legacy cases cover HTTP 0.x and 1.x; schema cases cover
 * HTTP 1.x. Compilation and execution check that response-test helper dependencies and body
 * handling agree with the server's selected HTTP version.
 */
internal class ServerProtocolTestHelpersTest {
    @ParameterizedTest
    @ValueSource(booleans = [false, true])
    fun `response assertions support the server HTTP version`(schemaSerde: Boolean) {
        val model =
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
            }])
            operation GetValue {
                input := {}
                output := { @httpHeader("x-value") header: String, value: String }
            }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                cargoCommand = "cargo test --quiet --lib --tests",
            ),
            testCoverage =
                if (schemaSerde) HttpTestType.Only(HttpTestVersion.HTTP_1_X) else HttpTestType.ALL,
        )
    }
}
