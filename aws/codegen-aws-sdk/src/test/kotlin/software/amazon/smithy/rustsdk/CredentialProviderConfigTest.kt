/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import SdkCodegenIntegrationTest
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

internal class CredentialProviderConfigTest {
    @Test
    fun `configuring credentials provider at operation level should work`() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, rustCrate ->
            val rc = ctx.runtimeConfig
            val codegenScope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "capture_request" to RuntimeType.captureRequest(rc),
                    "Credentials" to
                        AwsRuntimeType.awsCredentialTypesTestUtil(rc)
                            .resolve("Credentials"),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                )
            rustCrate.integrationTest("credentials_provider") {
                // per https://github.com/awslabs/aws-sdk-rust/issues/901
                tokioTest("configuring_credentials_provider_at_operation_level_should_work") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        let (http_client, _rx) = #{capture_request}(#{None});
                        let client_config = $moduleName::Config::builder()
                            .http_client(http_client)
                            .build();

                        let client = $moduleName::Client::from_conf(client_config);

                        let credentials = #{Credentials}::new(
                            "test",
                            "test",
                            #{None},
                            #{None},
                            "test",
                        );
                        let operation_config_override = $moduleName::Config::builder()
                            .credentials_provider(credentials.clone())
                            .region(#{Region}::new("us-west-2"));

                        let _ = client
                            .some_operation()
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

    @Test
    fun `configuring credentials provider on builder should replace what was previously set`() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, rustCrate ->
            val rc = ctx.runtimeConfig
            val codegenScope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "capture_request" to RuntimeType.captureRequest(rc),
                    "Credentials" to
                        AwsRuntimeType.awsCredentialTypesTestUtil(rc)
                            .resolve("Credentials"),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                    "SdkConfig" to AwsRuntimeType.awsTypes(rc).resolve("sdk_config::SdkConfig"),
                    "SharedCredentialsProvider" to
                        AwsRuntimeType.awsCredentialTypes(rc)
                            .resolve("provider::SharedCredentialsProvider"),
                )
            rustCrate.integrationTest("credentials_provider") {
                // per https://github.com/awslabs/aws-sdk-rust/issues/973
                tokioTest("configuring_credentials_provider_on_builder_should_replace_what_was_previously_set") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        let (http_client, rx) = #{capture_request}(#{None});

                        let replace_me = #{Credentials}::new(
                            "replace_me",
                            "replace_me",
                            #{None},
                            #{None},
                            "replace_me",
                        );
                        let sdk_config = #{SdkConfig}::builder()
                            .credentials_provider(
                                #{SharedCredentialsProvider}::new(replace_me),
                            )
                            .region(#{Region}::new("us-west-2"))
                            .build();

                        let expected = #{Credentials}::new(
                            "expected_credential",
                            "expected_credential",
                            #{None},
                            #{None},
                            "expected_credential",
                        );
                        let conf = $moduleName::config::Builder::from(&sdk_config)
                            .http_client(http_client)
                            .credentials_provider(expected)
                            .build();

                        let client = $moduleName::Client::from_conf(conf);

                        let _ = client
                            .neat_operation()
                            .send()
                            .await
                            .expect("success");

                        let req = rx.expect_request();
                        let auth_header = req.headers().get("AUTHORIZATION").unwrap();
                        assert!(auth_header.contains("expected_credential"), "{auth_header}");
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
