/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations.serde

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.CratesIo
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

/**
 * The serde decorators for the client and the server are _identical_, but they live in separate Gradle projects.
 * There's no point in testing everything twice, since the code is the same. Hence this file just contains a simple
 * smoke test to ensure we don't disable the client decorator _somehow_; all tests testing the decorator's logic should
 * live in `SerdeDecoratorTest` of the `codegen-server` project instead.
 */
class SerdeDecoratorTest {
    @Test
    fun `smoke test`() {
        val model =
            """
            namespace com.example
            use smithy.rust#serde
            use aws.protocols#awsJson1_0
            
            @awsJson1_0
            @serde
            service MyResourceService {
                resources: [MyResource]
            }
            
            resource MyResource {
                read: ReadMyResource
            }
            
            @readonly
            operation ReadMyResource {
                input := { }
            }
        """.asSmithyModel(smithyVersion = "2")

        val params =
            IntegrationTestParams(cargoCommand = "cargo test --all-features", service = "com.example#MyResourceService")
        clientIntegrationTest(model, params = params) { ctx, crate ->
            val codegenScope =
                arrayOf(
                    "crate" to RustType.Opaque(ctx.moduleUseName()),
                    "serde_json" to CargoDependency("serde_json", CratesIo("1")).toDevDependency().toType(),
                    // we need the derive feature
                    "serde" to CargoDependency.Serde.toDevDependency().toType(),
                )

            crate.integrationTest("test_serde") {
                unitTest("input_serialized") {
                    rustTemplate(
                        """
                        use #{crate}::operation::read_my_resource::ReadMyResourceInput;
                        use #{crate}::serde::*;
                        let input = ReadMyResourceInput::builder().build().unwrap();
                        let settings = SerializationSettings::default();
                        let _serialized = #{serde_json}::to_string(&input.serialize_ref(&settings)).expect("failed to serialize");
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
