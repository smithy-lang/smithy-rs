/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.client

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.testutil.TestCodegenSettings
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class CustomizableOperationGeneratorTest {
    val model = """
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
                        let connector = #{TestConnection}::<#{SdkBody}>::new(Vec::new());
                        let config = $moduleName::Config::builder()
                            .endpoint_resolver("http://localhost:1234")
                            #{set_http_connector}
                            .build();
                        let smithy_client = aws_smithy_client::Builder::new()
                            .connector(connector.clone())
                            .middleware_fn(|r| r)
                            .build_dyn();
                        let client = $moduleName::Client::with_config(smithy_client, config);
                        check_send_and_sync(client.say_hello().customize());
                    }
                    """,
                    "TestConnection" to CargoDependency.smithyClient(codegenContext.runtimeConfig)
                        .toDevDependency()
                        .withFeature("test-util").toType()
                        .resolve("test_connection::TestConnection"),
                    "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                    "set_http_connector" to writable {
                        if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
                            rust(".http_connector(connector.clone())")
                        }
                    },
                )
            }
        }
        clientIntegrationTest(model, TestCodegenSettings.middlewareModeTestParams, test = test)
        clientIntegrationTest(
            model,
            TestCodegenSettings.orchestratorModeTestParams,
            test = test,
        )
    }
}
