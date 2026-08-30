/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
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

        val testDirs =
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

        // test the generated metadata for all generated projects (both HTTP 0.x and HTTP 1.x)
        testDirs.forEach { generatedServer ->
            val cargoToml = generatedServer.path.resolve("Cargo.toml").readText()
            assert(cargoToml.contains("codegen-version =")) { cargoToml }
            assert(cargoToml.contains("protocol = \"aws.protocols#restJson1\"")) { cargoToml }
        }
    }

    @Test
    fun `schema serde flag generates categorized schema modules`() {
        val model =
            """
            ${'$'}version: "2"

            namespace com.aws.example.schema

            use aws.protocols#restJson1

            @restJson1
            service SchemaService {
                version: "2024-08-29"
                operations: [Echo]
            }

            @http(uri: "/echo/{name}", method: "POST")
            operation Echo {
                input := {
                    @required
                    @httpLabel
                    name: String

                    nested: Nested
                }
                output := {
                    nested: Nested
                }
                errors: [BadThing]
            }

            structure Nested {
                message: String
            }

            @error("client")
            @httpError(400)
            structure BadThing {
                message: String
            }
            """.asSmithyModel()

        val generatedServers =
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        ObjectNode.builder()
                            .withMember(
                                "codegen",
                                ObjectNode.builder()
                                    .withMember(ServerCodegenConfig.HTTP_1X_CONFIG_KEY, true)
                                    .withMember(ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY, true)
                                    .build(),
                            )
                            .build(),
                ),
            ) { _, _ -> }

        generatedServers.forEach { generatedServer ->
            val src = generatedServer.path.resolve("src")
            val inputSchema = src.resolve("schema/input/shape_echo_input.rs")
            val outputSchema = src.resolve("schema/output/shape_echo_output.rs")
            val errorSchema = src.resolve("schema/error/shape_bad_thing.rs")
            val modelSchema = src.resolve("schema/model/shape_nested.rs")
            val operationsSchema = src.resolve("schema/operations.rs")
            val serviceSchema = src.resolve("schema/service.rs")

            assert(inputSchema.toFile().exists()) { "missing $inputSchema" }
            assert(outputSchema.toFile().exists()) { "missing $outputSchema" }
            assert(errorSchema.toFile().exists()) { "missing $errorSchema" }
            assert(modelSchema.toFile().exists()) { "missing $modelSchema" }
            assert(operationsSchema.toFile().exists()) { "missing $operationsSchema" }
            assert(serviceSchema.toFile().exists()) { "missing $serviceSchema" }
            assert(inputSchema.readText().contains("ECHO_INPUT_SCHEMA")) { inputSchema.readText() }
            assert(operationsSchema.readText().contains("OperationSchema::new")) { operationsSchema.readText() }
            assert(operationsSchema.readText().contains("crate::schema::input::shape_echo_input::ECHO_INPUT")) {
                operationsSchema.readText()
            }
            assert(serviceSchema.readText().contains("ServiceSchema::new")) { serviceSchema.readText() }
            assert(serviceSchema.readText().contains("&crate::schema::operations::ECHO")) { serviceSchema.readText() }
        }
    }
}
