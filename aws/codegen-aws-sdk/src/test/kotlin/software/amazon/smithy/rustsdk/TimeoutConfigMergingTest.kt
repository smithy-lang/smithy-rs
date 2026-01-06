/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import SdkCodegenIntegrationTest
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class TimeoutConfigMergingTest {
    @Test
    fun testTimeoutSettingsProperlyMerged() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, crate ->
            val name = ctx.moduleUseName()
            crate.integrationTest("timeout_settings_properly_merged") {
                rustTemplate(
                    """

                    use $name::Client;
                    use aws_smithy_runtime::test_util::capture_test_logs::capture_test_logs;
                    use aws_smithy_runtime_api::box_error::BoxError;
                    use aws_smithy_runtime_api::client::interceptors::context::BeforeTransmitInterceptorContextRef;
                    use aws_smithy_runtime_api::client::interceptors::Intercept;
                    use aws_smithy_runtime_api::client::runtime_components::RuntimeComponents;
                    use aws_smithy_runtime::client::http::test_util::infallible_client_fn;
                    use aws_smithy_types::config_bag::ConfigBag;
                    use aws_smithy_types::timeout::TimeoutConfig;
                    use aws_smithy_types::body::SdkBody;
                    use aws_types::SdkConfig;
                    use std::sync::Arc;
                    use std::sync::Mutex;
                    use std::time::Duration;

                    ##[derive(Debug, Clone)]
                    struct CaptureConfigInterceptor {
                        timeout_config: Arc<Mutex<Option<TimeoutConfig>>>,
                    }

                    impl Intercept for CaptureConfigInterceptor {
                        fn name(&self) -> &'static str {
                            "capture config interceptor"
                        }

                        fn read_before_attempt(
                            &self,
                            _context: &BeforeTransmitInterceptorContextRef<'_>,
                            _runtime_components: &RuntimeComponents,
                            cfg: &mut ConfigBag,
                        ) -> Result<(), BoxError> {
                            *self.timeout_config.lock().unwrap() = cfg.load::<TimeoutConfig>().cloned();
                            Ok(())
                        }
                    }
                    #{tokio_test}
                    ##[allow(deprecated)]
                    async fn test_all_timeouts() {
                        let (_logs, _guard) = capture_test_logs();
                        let connect_timeout = Duration::from_secs(1);
                        let read_timeout = Duration::from_secs(2);
                        let operation_attempt = Duration::from_secs(3);
                        let operation = Duration::from_secs(4);
                        let http_client = infallible_client_fn(|_req| http::Response::builder().body(SdkBody::empty()).unwrap());
                        let sdk_config = SdkConfig::builder()
                            .behavior_version(aws_smithy_runtime_api::client::behavior_version::BehaviorVersion::v2025_01_17())
                            .timeout_config(
                                TimeoutConfig::builder()
                                    .connect_timeout(connect_timeout)
                                    .build(),
                            )
                            .http_client(http_client)
                            .build();
                        let client_config = $name::config::Builder::from(&sdk_config)
                            .timeout_config(TimeoutConfig::builder().read_timeout(read_timeout).build())
                            .build();
                        let client = Client::from_conf(client_config);
                        let interceptor = CaptureConfigInterceptor {
                            timeout_config: Default::default(),
                        };
                        let _err = client
                            .some_operation()
                            .customize()
                            .config_override(
                                $name::Config::builder().timeout_config(
                                    TimeoutConfig::builder()
                                        .operation_attempt_timeout(operation_attempt)
                                        .operation_timeout(operation)
                                        .build(),
                                ),
                            )
                            .interceptor(interceptor.clone())
                            .send()
                            .await;
                        let _ = dbg!(_err);
                        assert_eq!(
                            interceptor
                                .timeout_config
                                .lock()
                                .unwrap()
                                .as_ref()
                                .expect("timeout config not set"),
                            &TimeoutConfig::builder()
                                .operation_timeout(operation)
                                .operation_attempt_timeout(operation_attempt)
                                .read_timeout(read_timeout)
                                .connect_timeout(connect_timeout)
                                .build(),
                            "full set of timeouts set from all three sources."
                        );

                        // disable timeouts
                        let _err = client
                            .some_operation()
                            .customize()
                            .config_override(
                                $name::Config::builder().timeout_config(
                                    TimeoutConfig::disabled(),
                                ),
                            )
                            .interceptor(interceptor.clone())
                            .send()
                            .await;
                        let _ = dbg!(_err);
                        assert_eq!(
                            interceptor
                                .timeout_config
                                .lock()
                                .unwrap()
                                .as_ref()
                                .expect("timeout config not set"),
                            &TimeoutConfig::disabled(),
                            "timeouts disabled by config override"
                        );

                        // override one field
                        let _err = client
                            .some_operation()
                            .customize()
                            .config_override(
                                $name::Config::builder().timeout_config(
                                    TimeoutConfig::builder().read_timeout(Duration::from_secs(10)).build(),
                                ),
                            )
                            .interceptor(interceptor.clone())
                            .send()
                            .await;
                        let _ = dbg!(_err);
                        assert_eq!(
                            interceptor
                                .timeout_config
                                .lock()
                                .unwrap()
                                .as_ref()
                                .expect("timeout config not set"),
                            &TimeoutConfig::builder()
                                .read_timeout(Duration::from_secs(10))
                                .connect_timeout(connect_timeout)
                                .disable_operation_attempt_timeout()
                                .disable_operation_timeout()
                                .build(),
                            "read timeout overridden"
                        );
                    }
                    """,
                    "tokio_test" to writable { Attribute.TokioTest.render(this) },
                )
            }
        }
    }
}
