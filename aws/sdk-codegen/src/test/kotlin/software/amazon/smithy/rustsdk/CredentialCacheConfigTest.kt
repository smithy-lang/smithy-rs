/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
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
        use aws.auth#sigv4
        use smithy.rules#endpointRuleSet

        @service(sdkId: "Some Value")
        @awsJson1_0
        @sigv4(name: "dontcare")
        @auth([sigv4])
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

        operation SayHello { input: TestInput }
        structure TestInput {
           foo: String,
        }
    """.asSmithyModel()

    @Test
    fun `config override for credentials`() {
        awsSdkIntegrationTest(model) { clientCodegenContext, rustCrate ->
            val runtimeConfig = clientCodegenContext.runtimeConfig
            val codegenScope = arrayOf(
                *RuntimeType.preludeScope,
                "Credentials" to AwsRuntimeType.awsCredentialTypesTestUtil(runtimeConfig)
                    .resolve("Credentials"),
                "CredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                    .resolve("cache::CredentialsCache"),
                "Region" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("region::Region"),
                "ReplayEvent" to CargoDependency.smithyRuntime(runtimeConfig)
                    .toDevDependency().withFeature("test-util").toType()
                    .resolve("client::http::test_util::ReplayEvent"),
                "RuntimePlugin" to RuntimeType.smithyRuntimeApi(runtimeConfig)
                    .resolve("client::runtime_plugin::RuntimePlugin"),
                "SdkBody" to RuntimeType.sdkBody(runtimeConfig),
                "SharedCredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig)
                    .resolve("cache::SharedCredentialsCache"),
                "StaticReplayClient" to CargoDependency.smithyRuntime(runtimeConfig)
                    .toDevDependency().withFeature("test-util").toType()
                    .resolve("client::http::test_util::StaticReplayClient"),
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

                tokioTest("test_specifying_credentials_provider_only_at_operation_level_should_work") {
                    // per https://github.com/awslabs/aws-sdk-rust/issues/901
                    rustTemplate(
                        """
                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .body(#{SdkBody}::from("request body"))
                                    .unwrap(),
                                http::Response::builder()
                                    .status(200)
                                    .body(#{SdkBody}::from("response"))
                                    .unwrap(),
                            )],
                        );
                        let client_config = crate::config::Config::builder()
                            .http_client(http_client)
                            .build();
                        let client = crate::client::Client::from_conf(client_config);

                        let credentials = #{Credentials}::new(
                            "test",
                            "test",
                            #{None},
                            #{None},
                            "test",
                        );
                        let operation_config_override = crate::config::Config::builder()
                            .credentials_cache(#{CredentialsCache}::no_caching())
                            .credentials_provider(credentials.clone())
                            .region(#{Region}::new("us-west-2"));

                        let _ = client
                            .say_hello()
                            .customize()
                            .config_override(operation_config_override)
                            .send()
                            .await
                            .expect("success");
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
