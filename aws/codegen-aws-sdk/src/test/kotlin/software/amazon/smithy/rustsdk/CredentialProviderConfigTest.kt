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

    @Test
    fun `config override credentials should not leak and should be cached across retries`() {
        awsSdkIntegrationTest(SdkCodegenIntegrationTest.model) { ctx, rustCrate ->
            val rc = ctx.runtimeConfig
            val codegenScope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "Credentials" to
                        AwsRuntimeType.awsCredentialTypesTestUtil(rc)
                            .resolve("Credentials"),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                    "ProvideCredentials" to
                        AwsRuntimeType.awsCredentialTypes(rc)
                            .resolve("provider::ProvideCredentials"),
                    "ProvideCredentialsFuture" to
                        AwsRuntimeType.awsCredentialTypes(rc)
                            .resolve("provider::future::ProvideCredentials"),
                    "RetryConfig" to
                        RuntimeType.smithyTypes(rc).resolve("retry::RetryConfig"),
                    "SharedAsyncSleep" to
                        RuntimeType.smithyAsync(rc).resolve("rt::sleep::SharedAsyncSleep"),
                    "TokioSleep" to
                        CargoDependency.smithyAsync(rc).withFeature("rt-tokio")
                            .toType().resolve("rt::sleep::TokioSleep"),
                )
            val moduleName = ctx.moduleUseName()
            rustCrate.integrationTest("credentials_provider") {
                addDependency(CargoDependency.TracingTest.toDevDependency())
                addDependency(CargoDependency.Tracing.toDevDependency())
                addDependency(CargoDependency.Tokio.toDevDependency().withFeature("test-util"))

                rustTemplate(
                    """
                    ##[tracing_test::traced_test]
                    ##[::tokio::test]
                    async fn config_override_credentials_should_not_grow_client_cache_partitions() {
                        use aws_smithy_runtime::client::http::test_util::infallible_client_fn;
                        use aws_smithy_types::body::SdkBody;

                        let http_client = infallible_client_fn(|_req| {
                            http::Response::builder().body(SdkBody::empty()).unwrap()
                        });
                        let client_config = $moduleName::Config::builder()
                            .http_client(http_client)
                            .credentials_provider(#{Credentials}::new("base", "base", #{None}, #{None}, "base"))
                            .region(#{Region}::new("us-west-2"))
                            .build();
                        let client = $moduleName::Client::from_conf(client_config);

                        // Make one call without config_override to establish the baseline partition
                        let _ = client.some_operation().send().await;

                        // Now make 10 calls, each with a different credentials provider via config_override
                        for i in 0..10 {
                            let creds = #{Credentials}::new(
                                format!("akid_{i}"), format!("secret_{i}"), #{None}, #{None}, "test",
                            );
                            let _ = client
                                .some_operation()
                                .customize()
                                .config_override($moduleName::Config::builder().credentials_provider(creds))
                                .send()
                                .await;
                        }

                        // The client-level identity cache should not have grown beyond the initial
                        // partition. Each config_override gets its own short-lived cache, so the
                        // client cache should still have partition_count=1.
                        // If the client cache grew, it would log partition_count=2 on the second
                        // override — which is the first sign of the leak this test guards against.
                        assert!(logs_contain("partition_count=1"));
                        assert!(!logs_contain("partition_count=2"));
                    }

                    ##[::tokio::test]
                    async fn config_override_credentials_are_cached_across_retries() {
                        use std::sync::{Arc, Mutex};
                        use aws_smithy_runtime::client::http::test_util::{ReplayEvent, StaticReplayClient};
                        use aws_smithy_types::body::SdkBody;

                        // A credentials provider backed by a list. Each call to provide_credentials
                        // removes and returns the first element. If the cache works, only the first
                        // element should be consumed; the second should remain.
                        ##[derive(Debug, Clone)]
                        struct CredentialsList(Arc<Mutex<Vec<#{Credentials}>>>);
                        impl #{ProvideCredentials} for CredentialsList {
                            fn provide_credentials<'a>(&'a self) -> #{ProvideCredentialsFuture}<'a>
                            where Self: 'a,
                            {
                                let next = self.0.lock().unwrap().remove(0);
                                #{ProvideCredentialsFuture}::ready(#{Ok}(next))
                            }
                        }

                        let http_client = StaticReplayClient::new(vec![
                            // First attempt: 500 triggers retry
                            ReplayEvent::new(
                                http::Request::builder().body(SdkBody::from("")).unwrap(),
                                http::Response::builder().status(500).body(SdkBody::from("{}")).unwrap(),
                            ),
                            // Second attempt: 200 succeeds
                            ReplayEvent::new(
                                http::Request::builder().body(SdkBody::from("")).unwrap(),
                                http::Response::builder().status(200).body(SdkBody::from("{}")).unwrap(),
                            ),
                        ]);

                        let client_config = $moduleName::Config::builder()
                            .http_client(http_client)
                            .credentials_provider(#{Credentials}::new("base", "base", #{None}, #{None}, "base"))
                            .region(#{Region}::new("us-west-2"))
                            .retry_config(#{RetryConfig}::standard())
                            .sleep_impl(#{SharedAsyncSleep}::new(#{TokioSleep}::new()))
                            .build();
                        let client = $moduleName::Client::from_conf(client_config);

                        let creds_list = Arc::new(Mutex::new(vec![
                            #{Credentials}::new("first", "first", #{None}, #{None}, "first"),
                            #{Credentials}::new("second", "second", #{None}, #{None}, "second"),
                        ]));
                        let override_provider = CredentialsList(creds_list.clone());

                        let _ = client
                            .some_operation()
                            .customize()
                            .config_override(
                                $moduleName::Config::builder().credentials_provider(override_provider),
                            )
                            .send()
                            .await;

                        // Only the first credentials should have been consumed. The retry should
                        // have served the identity from the operation-scoped cache, leaving the
                        // second credentials untouched.
                        let remaining = creds_list.lock().unwrap();
                        assert_eq!(
                            1,
                            remaining.len(),
                            "expected one credentials remaining (retry should use cache), but found {}",
                            remaining.len(),
                        );
                        assert_eq!("second", remaining[0].access_key_id());
                    }
                    """,
                    *codegenScope,
                )
            }
        }
    }
}
