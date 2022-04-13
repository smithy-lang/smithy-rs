/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.protocol

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.CodegenVisitor
import software.amazon.smithy.rust.codegen.smithy.customize.CombinedCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.generatePluginContext
import software.amazon.smithy.rust.codegen.util.runCommand

class MakeOperationGeneratorTest {
    @Test
    fun `idempotent operations are marked to retry transient errors`() {
        val model = """
            namespace test
            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "2022-02-22",
                operations: [IdempotentOperation, ReadonlyOperation, NonIdempotentOperation],
            }

            structure SomeStruct {
            }

            @idempotent
            @http(uri: "/idempotent", method: "POST")
            operation IdempotentOperation {
                input: SomeStruct,
                output: SomeStruct,
            }

            @readonly
            @http(uri: "/readonly", method: "POST")
            operation ReadonlyOperation {
                input: SomeStruct,
                output: SomeStruct,
            }

            @http(uri: "/non-idempotent", method: "POST")
            operation NonIdempotentOperation {
                input: SomeStruct,
                output: SomeStruct,
            }
        """.asSmithyModel()

        val (pluginContext, testDir) = generatePluginContext(model)
        CodegenVisitor(
            pluginContext,
            CombinedCodegenDecorator.fromClasspath(pluginContext).withDecorator(object : RustCodegenDecorator {
                override val name: String = "TestMakeOperation"
                override val order: Byte = 0

                override fun crateManifestCustomizations(codegenContext: CodegenContext): ManifestCustomizations {
                    return mapOf(
                        "dependencies" to mapOf(
                            "tokio" to mapOf(
                                "version" to "1",
                                "features" to listOf("full")
                            )
                        )
                    )
                }

                override fun libRsCustomizations(
                    codegenContext: CodegenContext,
                    baseCustomizations: List<LibRsCustomization>
                ): List<LibRsCustomization> = baseCustomizations + listOf(
                    object : LibRsCustomization() {
                        override fun section(section: LibRsSection): Writable = writable {
                            when (section) {
                                is LibRsSection.Body -> rustBlock("##[tokio::test] async fn test()") {
                                    rust(
                                        """
                                        use crate::config::Config;
                                        use crate::input::*;
                                        use aws_smithy_client::retry::AllowOperationRetryOnTransientFailure;

                                        let config = Config::builder().build();
                                        let operation = IdempotentOperationInput::builder()
                                            .build()
                                            .expect("valid")
                                            .make_operation(&config)
                                            .await
                                            .expect("success");
                                        let properties = operation.properties();
                                        assert!(properties.get::<AllowOperationRetryOnTransientFailure>().is_some());

                                        let operation = ReadonlyOperationInput::builder()
                                            .build()
                                            .expect("valid")
                                            .make_operation(&config)
                                            .await
                                            .expect("success");
                                        let properties = operation.properties();
                                        assert!(properties.get::<AllowOperationRetryOnTransientFailure>().is_some());

                                        let operation = NonIdempotentOperationInput::builder()
                                            .build()
                                            .expect("valid")
                                            .make_operation(&config)
                                            .await
                                            .expect("success");
                                        let properties = operation.properties();
                                        assert!(properties.get::<AllowOperationRetryOnTransientFailure>().is_none());
                                        """
                                    )
                                }
                                else -> emptySection
                            }
                        }
                    }
                )
            })
        ).execute()
        "cargo test".runCommand(testDir)
    }
}
