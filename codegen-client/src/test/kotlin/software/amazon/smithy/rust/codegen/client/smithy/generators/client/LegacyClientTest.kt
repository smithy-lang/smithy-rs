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
        println("=== Cargo.toml (includeLegacyClient=true, default) ===")
        println(cargoToml)
        println("=== END ===")
        assert(cargoToml.contains("rustls = [\"aws-smithy-runtime/tls-rustls\"]")) {
            "Expected Cargo.toml to contain 'rustls' feature by default, but it didn't.\n$cargoToml"
        }
        assert(cargoToml.contains("\"aws-smithy-runtime/legacy-test-util\"")) {
            "Expected Cargo.toml to contain 'aws-smithy-runtime/legacy-test-util' in test-util deps by default.\n$cargoToml"
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
        println("=== Cargo.toml (includeLegacyClient=false) ===")
        println(cargoToml)
        println("=== END ===")
        assert(!cargoToml.contains("rustls = [\"aws-smithy-runtime/tls-rustls\"]")) {
            "Expected Cargo.toml to NOT contain 'rustls' feature when includeLegacyClient is false.\n$cargoToml"
        }
        assert(!cargoToml.contains("\"aws-smithy-runtime/legacy-test-util\"")) {
            "Expected Cargo.toml to NOT contain 'aws-smithy-runtime/legacy-test-util' when includeLegacyClient is false.\n$cargoToml"
        }
    }
}
