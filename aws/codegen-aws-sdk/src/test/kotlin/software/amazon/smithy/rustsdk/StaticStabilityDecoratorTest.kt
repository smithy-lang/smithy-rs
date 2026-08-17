/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import SdkCodegenIntegrationTest
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

internal class StaticStabilityDecoratorTest {
    @Test
    fun `bare AWS client defaults to the static-stability cache at v2026_08_01`() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, rustCrate ->
            val rc = ctx.runtimeConfig
            val moduleName = ctx.moduleUseName()
            val codegenScope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "Credentials" to AwsRuntimeType.awsCredentialTypesTestUtil(rc).resolve("Credentials"),
                    "StaticStabilityEligible" to
                        AwsRuntimeType.awsCredentialTypes(rc).resolve("StaticStabilityEligible"),
                    "ProvideCredentials" to
                        AwsRuntimeType.awsCredentialTypes(rc).resolve("provider::ProvideCredentials"),
                    "ProvideCredentialsFuture" to
                        AwsRuntimeType.awsCredentialTypes(rc).resolve("provider::future::ProvideCredentials"),
                    "CredentialsError" to
                        AwsRuntimeType.awsCredentialTypes(rc).resolve("provider::error::CredentialsError"),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                    "BehaviorVersion" to
                        RuntimeType.smithyRuntimeApiClient(rc).resolve("client::behavior_version::BehaviorVersion"),
                    "StaticTimeSource" to RuntimeType.smithyAsync(rc).resolve("time::StaticTimeSource"),
                    "SharedAsyncSleep" to RuntimeType.smithyAsync(rc).resolve("rt::sleep::SharedAsyncSleep"),
                    "TokioSleep" to
                        CargoDependency.smithyAsync(rc).withFeature("rt-tokio").toType()
                            .resolve("rt::sleep::TokioSleep"),
                )
            rustCrate.integrationTest("static_stability_cache_default") {
                addDependency(CargoDependency.Tokio.toDevDependency().withFeature("test-util"))
                rustTemplate(
                    """
                    use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
                    use aws_smithy_types::body::SdkBody;
                    use std::sync::{Arc, Mutex};
                    use std::time::{Duration, UNIX_EPOCH};

                    // Vends one eligible credential, then fails every later refresh.
                    ##[derive(Debug, Clone)]
                    struct OnceThenFail(Arc<Mutex<Option<#{Credentials}>>>);
                    impl #{ProvideCredentials} for OnceThenFail {
                        fn provide_credentials<'a>(&'a self) -> #{ProvideCredentialsFuture}<'a>
                        where
                            Self: 'a,
                        {
                            let next = self.0.lock().unwrap().take();
                            #{ProvideCredentialsFuture}::ready(match next {
                                #{Some}(creds) => #{Ok}(creds),
                                #{None} => #{Err}(#{CredentialsError}::provider_error("source unavailable")),
                            })
                        }
                    }

                    // Returns whether a second call still succeeds after the refresh fails.
                    async fn second_call_succeeds(bv: #{BehaviorVersion}) -> bool {
                        let http_client = StaticReplayClient::new(vec![
                            ReplayEvent::new(
                                http::Request::builder().body(SdkBody::from("")).unwrap(),
                                http::Response::builder().status(200).body(SdkBody::from("{}")).unwrap(),
                            ),
                            ReplayEvent::new(
                                http::Request::builder().body(SdkBody::from("")).unwrap(),
                                http::Response::builder().status(200).body(SdkBody::from("{}")).unwrap(),
                            ),
                        ]);

                        // Fixed clock; the seed expires 1s out, so the second call is already in the
                        // mandatory-refresh window and must contact the (now-failing) source.
                        let now = UNIX_EPOCH + Duration::from_secs(1000);
                        let mut seed = #{Credentials}::new(
                            "AKIDSTATIC",
                            "secret",
                            #{None},
                            #{Some}(now + Duration::from_secs(1)),
                            "test",
                        );
                        seed.set_property(#{StaticStabilityEligible});

                        let config = $moduleName::Config::builder()
                            .behavior_version(bv)
                            .http_client(http_client)
                            .time_source(#{StaticTimeSource}::new(now))
                            .sleep_impl(#{SharedAsyncSleep}::new(#{TokioSleep}::new()))
                            .credentials_provider(OnceThenFail(Arc::new(Mutex::new(#{Some}(seed)))))
                            .region(#{Region}::new("us-west-2"))
                            .build();
                        let client = $moduleName::Client::from_conf(config);

                        client
                            .some_operation()
                            .send()
                            .await
                            .expect("first call caches credentials");
                        client.some_operation().send().await.is_ok()
                    }

                    ##[::tokio::test]
                    async fn static_stability_cache_serves_cached_on_refresh_failure() {
                        assert!(
                            second_call_succeeds(#{BehaviorVersion}::v2026_08_01()).await,
                            "at v2026_08_01 the static-stability cache should serve the cached credential",
                        );
                    }

                    ##[allow(deprecated)]
                    ##[::tokio::test]
                    async fn older_behavior_version_errors_on_refresh_failure() {
                        assert!(
                            !second_call_succeeds(#{BehaviorVersion}::v2024_03_28()).await,
                            "without the static-stability cache a failed refresh should error",
                        );
                    }
                    """,
                    *codegenScope,
                )
            }

            rustCrate.integrationTest("credential_auth_failure_interceptor") {
                addDependency(CargoDependency.Tokio.toDevDependency().withFeature("test-util"))
                rustTemplate(
                    """
                    use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
                    use aws_smithy_types::body::SdkBody;
                    use std::sync::{Arc, Mutex};
                    use std::time::{Duration, SystemTime, UNIX_EPOCH};

                    // Vends a fresh, non-expired, eligible credential on every call, counting calls.
                    ##[derive(Debug, Clone)]
                    struct CountingProvider {
                        calls: Arc<Mutex<usize>>,
                        now: SystemTime,
                    }
                    impl #{ProvideCredentials} for CountingProvider {
                        fn provide_credentials<'a>(&'a self) -> #{ProvideCredentialsFuture}<'a>
                        where
                            Self: 'a,
                        {
                            let mut calls = self.calls.lock().unwrap();
                            *calls += 1;
                            let mut creds = #{Credentials}::new(
                                format!("AKID{}", *calls),
                                "secret",
                                #{None},
                                #{Some}(self.now + Duration::from_secs(3600)),
                                "test",
                            );
                            creds.set_property(#{StaticStabilityEligible});
                            #{ProvideCredentialsFuture}::ready(#{Ok}(creds))
                        }
                    }

                    // A target-service `ExpiredToken` rejection invalidates the signing identity via
                    // the per-operation interceptor, so the next call must re-resolve credentials.
                    ##[::tokio::test]
                    async fn expired_token_invalidates_and_reresolves() {
                        let now = UNIX_EPOCH + Duration::from_secs(1000);
                        let calls = Arc::new(Mutex::new(0usize));

                        let http_client = StaticReplayClient::new(vec![
                            // First attempt is rejected with `ExpiredToken`.
                            ReplayEvent::new(
                                http::Request::builder().body(SdkBody::from("")).unwrap(),
                                http::Response::builder()
                                    .status(403)
                                    .header("x-amzn-errortype", "ExpiredToken")
                                    .body(SdkBody::from("{}"))
                                    .unwrap(),
                            ),
                            // Second attempt succeeds with the re-resolved credential.
                            ReplayEvent::new(
                                http::Request::builder().body(SdkBody::from("")).unwrap(),
                                http::Response::builder().status(200).body(SdkBody::from("{}")).unwrap(),
                            ),
                        ]);

                        let config = $moduleName::Config::builder()
                            .behavior_version(#{BehaviorVersion}::v2026_08_01())
                            .http_client(http_client)
                            .time_source(#{StaticTimeSource}::new(now))
                            .sleep_impl(#{SharedAsyncSleep}::new(#{TokioSleep}::new()))
                            .credentials_provider(CountingProvider { calls: calls.clone(), now })
                            .region(#{Region}::new("us-west-2"))
                            .build();
                        let client = $moduleName::Client::from_conf(config);

                        // The rejected request surfaces as an error and is not retried.
                        let first = client.some_operation().send().await;
                        assert!(first.is_err(), "the ExpiredToken response should surface as an error");

                        // Invalidation forces the next resolution to contact the provider again.
                        client
                            .some_operation()
                            .send()
                            .await
                            .expect("second call should succeed after re-resolution");

                        assert_eq!(
                            *calls.lock().unwrap(),
                            2,
                            "invalidation must force credential re-resolution",
                        );
                    }
                    """,
                    *codegenScope,
                )
            }
        }
    }
}
