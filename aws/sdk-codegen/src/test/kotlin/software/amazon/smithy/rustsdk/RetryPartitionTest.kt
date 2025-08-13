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
                        let expected = r##"token bucket for RetryPartition { inner: Default("dontcare-us-west-2") } added to config bag"##;
                        assert!(log_contents.contains(expected));

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
                        let expected = r##"token bucket for RetryPartition { inner: Default("user-partition") } added to config bag"##;
                        assert!(log_contents.contains(expected));

                        """,
                        *codegenScope,
                        "RetryPartition" to RuntimeType.smithyRuntime(ctx.runtimeConfig).resolve("client::retries::RetryPartition"),
                    )
                }
            }
        }
    }

    // This test doesn't need to be in "sdk-codegen" but since "default retry partition" test was initially here,
    // it is added to this file for consistency.
    @Test
    fun `custom retry partition`() {
        awsSdkIntegrationTest(
            SdkCodegenIntegrationTest.model,
        ) { ctx, crate ->
            val codegenScope =
                arrayOf(
                    "BeforeTransmitInterceptorContextRef" to RuntimeType.beforeTransmitInterceptorContextRef(ctx.runtimeConfig),
                    "BoxError" to RuntimeType.boxError(ctx.runtimeConfig),
                    "capture_test_logs" to
                        CargoDependency.smithyRuntimeTestUtil(ctx.runtimeConfig).toType()
                            .resolve("test_util::capture_test_logs::capture_test_logs"),
                    "capture_request" to RuntimeType.captureRequest(ctx.runtimeConfig),
                    "ClientRateLimiter" to RuntimeType.smithyRuntime(ctx.runtimeConfig).resolve("client::retries::ClientRateLimiter"),
                    "ConfigBag" to RuntimeType.configBag(ctx.runtimeConfig),
                    "Intercept" to RuntimeType.intercept(ctx.runtimeConfig),
                    "RetryConfig" to RuntimeType.smithyTypes(ctx.runtimeConfig).resolve("retry::RetryConfig"),
                    "RetryPartition" to RuntimeType.smithyRuntime(ctx.runtimeConfig).resolve("client::retries::RetryPartition"),
                    "RuntimeComponents" to RuntimeType.runtimeComponents(ctx.runtimeConfig),
                    "TokenBucket" to RuntimeType.smithyRuntime(ctx.runtimeConfig).resolve("client::retries::TokenBucket"),
                )
            crate.integrationTest("custom_retry_partition") {
                tokioTest("test_custom_token_bucket") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        use std::sync::{Arc, atomic::{AtomicU32, Ordering}};
                        use $moduleName::{Client, Config};

                        ##[derive(Clone, Debug, Default)]
                        struct TestInterceptor {
                            called: Arc<AtomicU32>,
                        }
                        impl #{Intercept} for TestInterceptor {
                            fn name(&self) -> &'static str {
                                "TestInterceptor"
                            }
                            fn read_before_attempt(
                                &self,
                                _context: &#{BeforeTransmitInterceptorContextRef}<'_>,
                                _runtime_components: &#{RuntimeComponents},
                                cfg: &mut #{ConfigBag},
                            ) -> Result<(), #{BoxError}> {
                                self.called.fetch_add(1, Ordering::Relaxed);
                                let token_bucket = cfg.load::<#{TokenBucket}>().unwrap();
                                let expected = format!("permits: {}", tokio::sync::Semaphore::MAX_PERMITS);
                                assert!(
                                    format!("{token_bucket:?}").contains(&expected),
                                    "Expected debug output to contain `{expected}`, but got: {token_bucket:?}"
                                );
                                Ok(())
                            }
                        }

                        let (http_client, _) = #{capture_request}(None);
                        let test_interceptor = TestInterceptor::default();
                        let client_config = Config::builder()
                            .interceptor(test_interceptor.clone())
                            .retry_partition(#{RetryPartition}::custom("test")
                                .token_bucket(#{TokenBucket}::unlimited())
                                .build()
                            )
                            .http_client(http_client)
                            .build();

                        let client = Client::from_conf(client_config);
                        let _ = client.some_operation().send().await;

                        assert!(
                            test_interceptor.called.load(Ordering::Relaxed) == 1,
                            "the interceptor should have been called"
                        );
                        """,
                        *codegenScope,
                    )
                }

                tokioTest("test_custom_client_rate_limiter") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        use $moduleName::{Client, Config};

                        let (_logs, logs_rx) = #{capture_test_logs}();
                        let (http_client, _) = #{capture_request}(None);
                        let bucket_capacity = 0.5; // Should be less than INITIAL_REQUEST_COST (1.0)
                        let client_config = Config::builder()
                            .retry_partition(#{RetryPartition}::custom("test")
                                .client_rate_limiter(
                                    #{ClientRateLimiter}::builder()
                                        .enable_throttling(true)
                                        .current_bucket_capacity(bucket_capacity)
                                        .build()
                                )
                                .build()
                            )
                            .retry_config(#{RetryConfig}::adaptive())
                            .http_client(http_client)
                            .build();

                        let client = Client::from_conf(client_config);
                        let _ = client.some_operation().send().await;

                        let log_contents = logs_rx.contents();
                        assert!(log_contents.contains("client rate limiter delayed a request"));
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
