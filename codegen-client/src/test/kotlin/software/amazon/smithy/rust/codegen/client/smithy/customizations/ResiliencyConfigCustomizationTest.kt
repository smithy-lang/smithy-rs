/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.BasicTestModels
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

internal class ResiliencyConfigCustomizationTest {
    @Test
    fun `generates a valid config`() {
        clientIntegrationTest(BasicTestModels.AwsJson10TestModel) { _, crate ->
            crate.unitTest("resiliency_fields") {
                rustTemplate(
                    """
                    let mut conf = crate::Config::builder();
                    conf.set_sleep_impl(None);
                    conf.set_retry_config(None);
                    """,
                )
            }
        }
    }

    @Test
    fun `custom token bucket should be used at runtime`() {
        clientIntegrationTest(
            BasicTestModels.AwsJson10TestModel,
            IntegrationTestParams(cargoCommand = "cargo test --features behavior-version-latest,test-util"),
        ) { ctx, crate ->
            crate.integrationTest("token_bucket") {
                tokioTest("test_custom_token_bucket") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        use aws_smithy_runtime::client::http::test_util::capture_request;
                        use aws_smithy_runtime_api::box_error::BoxError;
                        use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                        use aws_smithy_runtime::client::retries::TokenBucket;
                        use std::sync::atomic::{AtomicU32, Ordering};
                        use std::sync::Arc;
                        use $moduleName::{
                            config::interceptors::BeforeSerializationInterceptorContextRef,
                            config::Intercept,
                            {Client, Config},
                        };

                        ##[derive(Clone, Debug, Default)]
                        struct TestInterceptor {
                            called: Arc<AtomicU32>,
                        }
                        impl Intercept for TestInterceptor {
                            fn name(&self) -> &'static str {
                                "TestInterceptor"
                            }
                            fn read_before_serialization(
                                &self,
                                _context: &BeforeSerializationInterceptorContextRef<'_>,
                                _runtime_components: &RuntimeComponents,
                                cfg: &mut aws_smithy_types::config_bag::ConfigBag,
                            ) -> Result<(), BoxError> {
                                self.called.fetch_add(1, Ordering::Relaxed);
                                let token_bucket = cfg.load::<TokenBucket>().unwrap();
                                let expected = format!("permits: {}", tokio::sync::Semaphore::MAX_PERMITS);
                                assert!(
                                    format!("{token_bucket:?}").contains(&expected),
                                    "Expected debug output to contain `{expected}`, but got: {token_bucket:?}"
                                );
                                Ok(())
                            }
                        }

                        let (http_client, _) = capture_request(None);
                        let test_interceptor = TestInterceptor::default();
                        let client_config = Config::builder()
                            .interceptor(test_interceptor.clone())
                            .token_bucket(TokenBucket::unlimited())
                            .endpoint_url("http://localhost:1234/")
                            .http_client(http_client)
                            .build();

                        let client = Client::from_conf(client_config);
                        let _ = client.say_hello().send().await;

                        assert!(
                            test_interceptor.called.load(Ordering::Relaxed) == 1,
                            "the interceptor should have been called"
                        );
                        """,
                    )
                }
            }
        }
    }
}
