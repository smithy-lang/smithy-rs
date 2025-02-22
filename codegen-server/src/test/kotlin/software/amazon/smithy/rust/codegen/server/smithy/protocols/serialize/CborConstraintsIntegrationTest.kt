/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.serialize

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ModelProtocol
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.server.smithy.loadSmithyConstraintsModelForProtocol
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class CborConstraintsIntegrationTest {
    @Test
    fun `ensure CBOR implementation works for all constraint types`() {
        val (model, serviceShape) = loadSmithyConstraintsModelForProtocol(ModelProtocol.Rpcv2Cbor)
        // The test should compile; no further testing is required.
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShape.toString(),
                additionalSettings = ServerAdditionalSettings.builder().generateCodegenComments().toObjectNode(),
            ),
        ) { _, _ ->
        }
    }
}
