/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class RetryPartitionTest {
    @Test
    fun `default retry partition`() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, rustCrate ->
            val rc = ctx.runtimeConfig
            val codegenScope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "capture_request" to RuntimeType.captureRequest(rc),
                    "capture_test_logs" to
                        CargoDependency.smithyRuntimeTestUtil(rc).toType()
                            .resolve("test_util::capture_test_logs::capture_test_logs"),
                    "Credentials" to
                        AwsRuntimeType.awsCredentialTypesTestUtil(rc)
                            .resolve("Credentials"),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                )

            rustCrate.integrationTest("default_retry_partition") {
                tokioTest("default_retry_partition_includes_region") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        let (_logs, logs_rx) = #{capture_test_logs}();
                        let (http_client, _rx) = #{capture_request}(#{None});
                        let client_config = $moduleName::Config::builder()
                            .http_client(http_client)
                            .region(#{Region}::new("us-west-2"))
                            .credentials_provider(#{Credentials}::for_tests())
                            .build();

                        let client = $moduleName::Client::from_conf(client_config);

                        let _ = client
                            .some_operation()
                            .send()
                            .await
                            .expect("success");

                        let log_contents = logs_rx.contents();
                        assert!(log_contents.contains("token bucket for RetryPartition { name: \"dontcare-us-west-2\" } added to config bag"));

                        """,
                        *codegenScope,
                    )
                }

                tokioTest("user_config_retry_partition") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        let (_logs, logs_rx) = #{capture_test_logs}();
                        let (http_client, _rx) = #{capture_request}(#{None});
                        let client_config = $moduleName::Config::builder()
                            .http_client(http_client)
                            .region(#{Region}::new("us-west-2"))
                            .credentials_provider(#{Credentials}::for_tests())
                            .retry_partition(#{RetryPartition}::new("user-partition"))
                            .build();

                        let client = $moduleName::Client::from_conf(client_config);

                        let _ = client
                            .some_operation()
                            .send()
                            .await
                            .expect("success");

                        let log_contents = logs_rx.contents();
                        assert!(log_contents.contains("token bucket for RetryPartition { name: \"user-partition\" } added to config bag"));

                        """,
                        *codegenScope,
                        "RetryPartition" to RuntimeType.smithyRuntime(ctx.runtimeConfig).resolve("client::retries::RetryPartition"),
                    )
                }
            }
        }
    }
}
