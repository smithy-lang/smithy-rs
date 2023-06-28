/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

internal class CredentialCacheConfigTest {
    private val model = """
        namespace com.example
        use aws.protocols#awsJson1_0
        use aws.api#service
        use smithy.rules#endpointRuleSet

        @service(sdkId: "Some Value")
        @awsJson1_0
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{
                          "type": "endpoint",
                          "conditions": [{"fn": "isSet", "argv": [{"ref": "Region"}]}],
                          "endpoint": { "url": "https://example.com" }
                      }],
            "parameters": {
                "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
            }
        })
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
    fun `config override for credentials`() {
        awsSdkIntegrationTest(model, defaultToOrchestrator = true) { clientCodegenContext, rustCrate ->
            val runtimeConfig = clientCodegenContext.runtimeConfig
            val codegenScope = arrayOf(
                *RuntimeType.preludeScope,
                "Credentials" to AwsRuntimeType.awsCredentialTypesTestUtil(runtimeConfig)
                    .resolve("Credentials"),
                "CredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("cache::CredentialsCache"),
                "RuntimePlugin" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::runtime_plugin::RuntimePlugin"),
                "SharedCredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("cache::SharedCredentialsCache"),
            )
            rustCrate.withModule(ClientRustModule.config) {
                unitTest("test_overriding_credentials_provider_leads_to_shared_credentials_cache_in_layer") {
                    rustTemplate(
                        """
                        use #{RuntimePlugin};

                        let client_config = Config::builder().build();
                        let config_override =
                            Config::builder().credentials_provider(#{Credentials}::for_tests());
                        let sut = ConfigOverrideRuntimePlugin {
                            client_config: client_config.config().unwrap(),
                            config_override,
                        };
                        let sut_layer = sut.config().unwrap();
                        assert!(sut_layer
                            .load::<#{SharedCredentialsCache}>()
                            .is_some());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("test_overriding_credentials_cache_leads_to_shared_credentials_cache_in_layer") {
                    rustTemplate(
                        """
                        use #{RuntimePlugin};

                        // TODO(enableNewSmithyRuntimeCleanup): Should not need to provide a default once smithy-rs##2770
                        //  is resolved
                        let client_config = Config::builder()
                            .credentials_provider(#{Credentials}::for_tests())
                            .build();
                        let config_override = Config::builder()
                            .credentials_cache(#{CredentialsCache}::no_caching());
                        let sut = ConfigOverrideRuntimePlugin {
                            client_config: client_config.config().unwrap(),
                            config_override,
                        };
                        let sut_layer = sut.config().unwrap();
                        assert!(sut_layer
                            .load::<#{SharedCredentialsCache}>()
                            .is_some());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("test_overriding_cache_and_provider_leads_to_shared_credentials_cache_in_layer") {
                    rustTemplate(
                        """
                        use #{RuntimePlugin};

                        let client_config = Config::builder().build();
                        let config_override = Config::builder()
                            .credentials_cache(#{CredentialsCache}::lazy())
                            .credentials_provider(#{Credentials}::for_tests());
                        let sut = ConfigOverrideRuntimePlugin {
                            client_config: client_config.config().unwrap(),
                            config_override,
                        };
                        let sut_layer = sut.config().unwrap();
                        assert!(sut_layer
                            .load::<#{SharedCredentialsCache}>()
                            .is_some());
                        """,
                        *codegenScope,
                    )
                }

                unitTest("test_not_overriding_cache_and_provider_leads_to_no_shared_credentials_cache_in_layer") {
                    rustTemplate(
                        """
                        use #{RuntimePlugin};

                        let client_config = Config::builder().build();
                        let config_override = Config::builder();
                        let sut = ConfigOverrideRuntimePlugin {
                            client_config: client_config.config().unwrap(),
                            config_override,
                        };
                        let sut_layer = sut.config().unwrap();
                        assert!(sut_layer
                            .load::<#{SharedCredentialsCache}>()
                            .is_none());
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
