/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class CustomizableOperationGeneratorTest {
    val model =
        """
        namespace com.example
        use aws.protocols#awsJson1_0

        @awsJson1_0
        service HelloService {
            operations: [SayHello],
            version: "1"
        }

        @optionalAuth
        operation SayHello { input: TestInput }
        structure TestInput {
           foo: String,
        }
        """.asSmithyModel()

    @Test
    fun `CustomizableOperation is send and sync`() {
        val test: (ClientCodegenContext, RustCrate) -> Unit = { codegenContext, rustCrate ->
            rustCrate.integrationTest("customizable_operation_is_send_and_sync") {
                val moduleName = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    fn check_send_and_sync<T: Send + Sync>(_: T) {}

                    ##[test]
                    fn test() {
                        let config = $moduleName::Config::builder()
                            .http_client(#{NeverClient}::new())
                            .endpoint_url("http://localhost:1234")
                            .build();
                        let client = $moduleName::Client::from_conf(config);
                        check_send_and_sync(client.say_hello().customize());
                    }
                    """,
                    "NeverClient" to
                        CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig).toType()
                            .resolve("test_util::NeverClient"),
                )
            }
        }
        clientIntegrationTest(model, test = test)
    }
}
