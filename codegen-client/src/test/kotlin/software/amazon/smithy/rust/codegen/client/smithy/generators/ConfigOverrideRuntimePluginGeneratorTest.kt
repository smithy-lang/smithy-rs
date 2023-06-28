/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.testutil.TestCodegenSettings
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

internal class ConfigOverrideRuntimePluginGeneratorTest {
    private val model = """
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
    fun `layer for operation level config`() {
        clientIntegrationTest(
            model,
            params = IntegrationTestParams(additionalSettings = TestCodegenSettings.orchestratorMode()),
        ) { clientCodegenContext, rustCrate ->
            val runtimeConfig = clientCodegenContext.runtimeConfig
            val codegenScope = arrayOf(
                *preludeScope,
                "ConfigBagAccessors" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::config_bag_accessors::ConfigBagAccessors"),
                "RuntimePlugin" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::runtime_plugin::RuntimePlugin"),
            )
            rustCrate.withModule(ClientRustModule.config) {
                unitTest("test_config_override_layer_contains_endpoint_resolver") {
                    rustTemplate(
                        """
                        use #{ConfigBagAccessors};
                        use #{RuntimePlugin};

                        let client_config = Config::builder().build();
                        let config_override = Config::builder().endpoint_resolver("http://localhost:1234");
                        let sut = ConfigOverrideRuntimePlugin {
                            client_config: client_config.config().unwrap(),
                            config_override,
                        };
                        let sut_layer = sut.config().unwrap();
                        let _ = sut_layer.endpoint_resolver();
                        """,
                        *codegenScope,
                    )
                }

                unitTest("test_config_override_layer_contains_http_connection") {
                    rustTemplate(
                        """
                        use #{ConfigBagAccessors};
                        use #{RuntimePlugin};

                        let (conn, _) = #{capture_request}(None);
                        let client_config = Config::builder().build();
                        let config_override = Config::builder().http_connector(conn);
                        let sut = ConfigOverrideRuntimePlugin {
                            client_config: client_config.config().unwrap(),
                            config_override,
                        };
                        let sut_layer = sut.config().unwrap();
                        let _ = sut_layer.connection();
                        """,
                        *codegenScope,
                        "capture_request" to CargoDependency.smithyClientTestUtil(clientCodegenContext.runtimeConfig)
                            .toType().resolve("test_connection::capture_request"),
                    )
                }

                unitTest("test_config_override_layer_contains_retry_strategy") {
                    rustTemplate(
                        """
                        use #{ConfigBagAccessors};
                        use #{RuntimePlugin};

                        let client_config = Config::builder().build();
                        let config_override = Config::builder().retry_config(#{RetryConfig}::disabled());
                        let sut = ConfigOverrideRuntimePlugin {
                            client_config: client_config.config().unwrap(),
                            config_override,
                        };
                        let sut_layer = sut.config().unwrap();
                        assert!(sut_layer.retry_strategy().is_some());
                        """,
                        *codegenScope,
                        "RetryConfig" to RuntimeType.smithyTypes(clientCodegenContext.runtimeConfig)
                            .resolve("retry::RetryConfig"),
                    )
                }
            }
        }
    }
}
