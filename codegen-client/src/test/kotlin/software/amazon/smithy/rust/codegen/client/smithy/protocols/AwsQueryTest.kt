/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.RustCodegenPlugin
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.core.util.runCommand

class AwsQueryTest {
    private val model = """
        namespace test
        use aws.protocols#awsQuery

        @awsQuery
        @xmlNamespace(uri: "https://example.com/")
        service TestService {
            version: "2019-12-16",
            operations: [SomeOperation]
        }

        operation SomeOperation {
            input: SomeOperationInputOutput,
            output: SomeOperationInputOutput,
        }

        structure SomeOperationInputOutput {
            payload: String,
            a: String,
            b: Integer
        }
    """.asSmithyModel()

    @Test
    fun `generate an aws query service that compiles`() {
        val (pluginContext, testDir) = generatePluginContext(model)
        RustCodegenPlugin().execute(pluginContext)
        "cargo check".runCommand(testDir)
    }
}
