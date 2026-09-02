/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.testutil.ClientAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import kotlin.io.path.readText

class LegacyClientTest {
    private val model =
        """
        namespace com.example
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service TestService {
            operations: [TestOp],
            version: "1"
        }

        @optionalAuth
        operation TestOp { input: TestInput }
        structure TestInput { foo: String }
        """.asSmithyModel()

    @Test
    fun `rustls feature is included by default`() {
        val testDir =
            clientIntegrationTest(model) { _, _ -> }
        val cargoToml = testDir.resolve("Cargo.toml").readText()
        assert(cargoToml.contains("rustls = [\"aws-smithy-runtime/tls-rustls\"]")) {
            "Expected Cargo.toml to contain 'rustls' feature by default, but it didn't.\n$cargoToml"
        }
        assert(cargoToml.contains("legacy-test-util = [\"aws-smithy-runtime/legacy-test-util\"]")) {
            "Expected Cargo.toml to offer an opt-in 'legacy-test-util' feature by default.\n$cargoToml"
        }
        // `test-util` must not reach the legacy test utilities: `aws-smithy-runtime/test-util`
        // implies `legacy-test-util`, which would pull http 0.2.x back into the tree of anything
        // built with `--features test-util`.
        val testUtilLine = cargoToml.lines().single { it.startsWith("test-util = ") }
        assert(!testUtilLine.contains("aws-smithy-runtime/test-util")) {
            "Expected 'test-util' to not enable 'aws-smithy-runtime/test-util'.\n$testUtilLine"
        }
        assert(!testUtilLine.contains("legacy-test-util")) {
            "Expected 'test-util' to not enable 'legacy-test-util'.\n$testUtilLine"
        }
    }

    @Test
    fun `rustls feature is excluded when includeLegacyClient is false`() {
        val testDir =
            clientIntegrationTest(
                model,
                params =
                    IntegrationTestParams(
                        cargoCommand = "cargo check",
                        additionalSettings =
                            ClientAdditionalSettings.builder()
                                .includeLegacyClient(false)
                                .build()
                                .toObjectNode(),
                    ),
            ) { _, _ -> }
        val cargoToml = testDir.resolve("Cargo.toml").readText()
        assert(!cargoToml.contains("rustls = [\"aws-smithy-runtime/tls-rustls\"]")) {
            "Expected Cargo.toml to NOT contain 'rustls' feature when includeLegacyClient is false.\n$cargoToml"
        }
        assert(!cargoToml.contains("\"aws-smithy-runtime/legacy-test-util\"")) {
            "Expected Cargo.toml to NOT contain 'aws-smithy-runtime/legacy-test-util' when includeLegacyClient is false.\n$cargoToml"
        }
    }
}
