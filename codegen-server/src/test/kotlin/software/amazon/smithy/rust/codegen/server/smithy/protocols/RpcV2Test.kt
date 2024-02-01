/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

// TODO This won't be needed since we'll cover it with a proper integration test.
internal class RpcV2Test {
    val model = """
        ${"\$"}version: "2.0"

        namespace com.amazonaws.simple

        use smithy.protocols#rpcv2

        @rpcv2(format: ["cbor"])
        service RpcV2Service {
            version: "SomeVersion",
            operations: [RpcV2Operation],
        }

        @http(uri: "/operation", method: "POST")
        operation RpcV2Operation {
            input: OperationInputOutput
            output: OperationInputOutput
        }

        structure OperationInputOutput {
            message: String
        }
    """.asSmithyModel()

    @Test
    fun `generate a rpc v2 service that compiles`() {
        serverIntegrationTest(model) { _, _ -> }
    }
}
