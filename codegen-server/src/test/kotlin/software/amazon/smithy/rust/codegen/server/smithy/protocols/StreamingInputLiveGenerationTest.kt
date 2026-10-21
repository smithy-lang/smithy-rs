/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

internal class StreamingInputLiveGenerationTest {
    @Test
    fun `generate fresh event stream servers for socket comparisons`() {
        val runEnv = System.getenv("STREAMING_INPUT_RUN")
        assumeTrue(runEnv != null, "Run tools/schema-serde-compat/run_streaming_input.py")
        val root = Path.of(System.getenv("STREAMING_INPUT_ROOT"))
        val run = Path.of(runEnv)
        val model = Files.readString(root.resolve("codegen-core/common-test-models/rpcv2Cbor-extras.smithy")).asSmithyModel()
        for (schema in listOf(false, true)) {
            for (initial in listOf(false, true)) {
                val mode = if (schema) "schema" else "legacy"
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "smithy.protocoltests.rpcv2Cbor#RpcV2CborService",
                        overrideTestDir = run.resolve("$mode-${if (initial) "initial" else "no-initial"}").toFile(),
                        additionalSettings =
                            ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schema).alwaysSendEventStreamInitialResponse(initial).toObjectNode(),
                        command = {},
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ) { context, _ -> check(context.usesSchemaHttpSerde == schema) }
            }
        }
    }
}
