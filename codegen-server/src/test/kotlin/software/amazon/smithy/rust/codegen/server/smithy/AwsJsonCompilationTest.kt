/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.EnumSource
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Test to verify AwsJson protocols compile correctly with streaming operations.
 *
 * This test ensures that the ResponseRejection enum in aws_json protocol
 * has proper From implementations for BuildError, which is needed when
 * serializing HTTP payloads for streaming operations.
 */
internal class AwsJsonCompilationTest {
    @ParameterizedTest
    @EnumSource(
        value = ModelProtocol::class,
        names = ["AwsJson10", "AwsJson11"],
    )
    fun `AwsJson protocols should compile with streaming operations`(protocol: ModelProtocol) {
        val (model) = loadSmithyConstraintsModelForProtocol(protocol)
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings =
                    ServerAdditionalSettings.builder()
                        .publicConstrainedTypes(true)
                        .generateCodegenComments(true)
                        .toObjectNode(),
            ),
        ) { _, _ ->
            // Test passes if code generation and compilation succeed
        }
    }
}
