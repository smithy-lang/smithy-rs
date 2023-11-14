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
            val codegenScope = arrayOf(
                *RuntimeType.preludeScope,
                "capture_request" to RuntimeType.captureRequest(rc),
                "Credentials" to AwsRuntimeType.awsCredentialTypesTestUtil(rc)
                    .resolve("Credentials"),
                "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
            )
            rustCrate.integrationTest("credentials_provider") {
                // per https://github.com/awslabs/aws-sdk-rust/issues/901
                tokioTest("configuring_credentials_provider_at_operation_level_should_work") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        let (http_client, _rx) = #{capture_request}(None);
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
}
