/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
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
        awsSdkIntegrationTest(model, generateOrchestrator = true) { clientCodegenContext, rustCrate ->
            val runtimeConfig = clientCodegenContext.runtimeConfig
            val codegenScope = arrayOf(
                *RuntimeType.preludeScope,
                "Credentials" to AwsRuntimeType.awsCredentialTypesTestUtil(runtimeConfig)
                    .resolve("Credentials"),
                "CredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                    .resolve("cache::CredentialsCache"),
                "ProvideCachedCredentials" to AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                    .resolve("cache::ProvideCachedCredentials"),
                "RuntimePlugin" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                    .resolve("client::runtime_plugin::RuntimePlugin"),
                "SharedCredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                    .resolve("cache::SharedCredentialsCache"),
                "SharedCredentialsProvider" to AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                    .resolve("provider::SharedCredentialsProvider"),
            )
            rustCrate.testModule {
                unitTest(
                    "test_overriding_only_credentials_provider_should_panic",
                    additionalAttributes = listOf(Attribute.shouldPanic("also specify `.credentials_cache` when overriding credentials provider for the operation")),
                ) {
                    rustTemplate(
                        """
                        use #{RuntimePlugin};

                        let client_config = crate::config::Config::builder().build();
                        let config_override =
                            crate::config::Config::builder().credentials_provider(#{Credentials}::for_tests());
                        let sut = crate::config::ConfigOverrideRuntimePlugin::new(
                            config_override,
                            client_config.config,
                            &client_config.runtime_components,
                        );

                        // this should cause `panic!`
                        let _ = sut.config().unwrap();
                        """,
                        *codegenScope,
                    )
                }

                unitTest(
                    "test_overriding_only_credentials_cache_should_panic",
                    additionalAttributes = listOf(Attribute.shouldPanic("also specify `.credentials_provider` when overriding credentials cache for the operation")),
                ) {
                    rustTemplate(
                        """
                        use #{RuntimePlugin};

                        let client_config = crate::config::Config::builder().build();
                        let config_override = crate::config::Config::builder()
                            .credentials_cache(#{CredentialsCache}::no_caching());
                        let sut = crate::config::ConfigOverrideRuntimePlugin::new(
                            config_override,
                            client_config.config,
                            &client_config.runtime_components,
                        );

                        // this should cause `panic!`
                        let _ = sut.config().unwrap();
                        """,
                        *codegenScope,
                    )
                }

                tokioTest("test_overriding_cache_and_provider_leads_to_shared_credentials_cache_in_layer") {
                    rustTemplate(
                        """
                        use #{ProvideCachedCredentials};
                        use #{RuntimePlugin};

                        let client_config = crate::config::Config::builder()
                            .credentials_provider(#{Credentials}::for_tests())
                            .build();
                        let client_config_layer = client_config.config;

                        // make sure test credentials are set in the client config level
                        assert_eq!(#{Credentials}::for_tests(),
                            client_config_layer
                            .load::<#{SharedCredentialsCache}>()
                            .unwrap()
                            .provide_cached_credentials()
                            .await
                            .unwrap()
                        );

                        let credentials = #{Credentials}::new(
                            "test",
                            "test",
                            #{None},
                            #{None},
                            "test",
                        );
                        let config_override = crate::config::Config::builder()
                            .credentials_cache(#{CredentialsCache}::lazy())
                            .credentials_provider(credentials.clone());
                        let sut = crate::config::ConfigOverrideRuntimePlugin::new(
                            config_override,
                            client_config_layer,
                            &client_config.runtime_components,
                        );
                        let sut_layer = sut.config().unwrap();

                        // make sure `.provide_cached_credentials` returns credentials set through `config_override`
                        assert_eq!(credentials,
                            sut_layer
                            .load::<#{SharedCredentialsCache}>()
                            .unwrap()
                            .provide_cached_credentials()
                            .await
                            .unwrap()
                        );
                        """,
                        *codegenScope,
                    )
                }

                unitTest("test_not_overriding_cache_and_provider_leads_to_no_shared_credentials_cache_in_layer") {
                    rustTemplate(
                        """
                        use #{RuntimePlugin};

                        let client_config = crate::config::Config::builder().build();
                        let config_override = crate::config::Config::builder();
                        let sut = crate::config::ConfigOverrideRuntimePlugin::new(
                            config_override,
                            client_config.config,
                            &client_config.runtime_components,
                        );
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
