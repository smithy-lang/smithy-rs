/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File
import kotlin.io.path.readText

internal class ServerServiceGeneratorTest {
    /**
     * See <https://github.com/smithy-lang/smithy-rs/issues/3177>.
     */
    @Test
    fun `one should be able to return a built service from a function`() {
        val model = File("../codegen-core/common-test-models/simple.smithy").readText().asSmithyModel()

        var testDir =
            serverIntegrationTest(model) { _, rustCrate ->
                rustCrate.testModule {
                    // No actual tests: we just want to check that this compiles.
                    rust(
                        """
                    fn _build_service() -> crate::SimpleService {
                        let config = crate::SimpleServiceConfig::builder().build();
                        let service = crate::SimpleService::builder(config).build_unchecked();

                        service.boxed()
                    }
                    """,
                    )
                }
            }

        // test the generated metadata
        val cargoToml = testDir.resolve("Cargo.toml").readText()
        assert(cargoToml.contains("codegen-version =")) { cargoToml }
        assert(cargoToml.contains("protocol = \"aws.protocols#restJson1\"")) { cargoToml }
    }
}
