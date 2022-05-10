/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.smithy.RustCodegenPlugin
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.util.runCommand

internal class RestJsonTest {
    val model = """
        namespace test
        use aws.protocols#restJson1
        use aws.api#service
        use smithy.test#httpRequestTests
        use smithy.test#httpResponseTests

        /// A REST JSON service that sends JSON requests and responses.
        @service(sdkId: "Rest Json Protocol")
        @restJson1
        service RestJsonExtras {
            version: "2019-12-16",
            operations: [StringPayload]
        }

        @http(uri: "/StringPayload", method: "POST")
        operation StringPayload {
            input: StringPayloadInput,
            output: StringPayloadInput
        }

        structure StringPayloadInput {
            payload: String,
            a: String,
            b: Integer
        }
    """.asSmithyModel()

    @Test
    fun `generate a rest json service that compiles`() {
        val (pluginContext, testDir) = generatePluginContext(model)
        RustCodegenPlugin().execute(pluginContext)
        "cargo check".runCommand(testDir)
    }
}
