/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Generates services with operation names that could collide with generated descriptor and
 * error-table suffixes. It compiles both legacy and schema HTTP 1.x crates so duplicate Rust
 * names or references to the wrong operation marker fail at compile time.
 */
internal class OperationDescriptorNamesTest {
    @Test
    fun `operation descriptors cannot collide with error table suffixes`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.operationcollision
            use aws.protocols#restJson1
            @restJson1 service Names { version: "1", operations: [GetFoo, GetFooErrors] }
            @http(method: "POST", uri: "/foo")
            operation GetFoo { input := {}, output := {}, errors: [Denied] }
            @http(method: "POST", uri: "/errors")
            operation GetFooErrors { input := {}, output := {} }
            @error("client") structure Denied { message: String }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Default,
            )
        }
    }
}
