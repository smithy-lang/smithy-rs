/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class UserAgentDecoratorTest {
    companion object {
        // Can't use the dollar sign in a multiline string with doing it like this.
        private const val PREFIX = "\$version: \"2\""
        val model =
            """
            $PREFIX
            namespace test

            use aws.api#service
            use aws.auth#sigv4
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet

            @service(sdkId: "dontcare")
            @restJson1
            @sigv4(name: "dontcare")
            @auth([sigv4])
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
            })
            service TestService {
                version: "2023-01-01",
                operations: [SomeOperation]
            }

            @http(uri: "/SomeOperation", method: "GET")
            @optionalAuth
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput
            }

            @input
            structure SomeInput {}

            @output
            structure SomeOutput {}
            """.asSmithyModel()
    }

    @Test
    fun smokeTestSdkCodegen() {
        awsSdkIntegrationTest(model) { _, _ ->
            // it should compile
        }
    }

    @Test
    fun userAgentWorks() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("user-agent") {
                rustTemplate(
                    """
                    use $moduleName::config::{AppName, Credentials, Region, SharedCredentialsProvider};
                    use $moduleName::{Config, Client};
                    use #{capture_request};

                    ##[#{tokio}::test]
                    async fn user_agent_app_name() {
                        let (http_client, rcvr) = capture_request(None);
                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .app_name(AppName::new("test-app-name").expect("valid app name")) // set app name in config
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;

                        // verify app name made it to the user agent
                        let request = rcvr.expect_request();
                        let formatted = std::str::from_utf8(
                            request
                                .headers()
                                .get("x-amz-user-agent")
                                .unwrap()
                                .as_bytes(),
                        )
                        .unwrap();
                        assert!(
                            formatted.ends_with(" app/test-app-name"),
                            "'{}' didn't end with the app name",
                            formatted
                        );
                    }

                    ##[#{tokio}::test]
                    async fn user_agent_http_client() {
                        let (http_client, rcvr) = capture_request(None);
                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .app_name(AppName::new("test-app-name").expect("valid app name")) // set app name in config
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;

                        // verify app name made it to the user agent
                        let request = rcvr.expect_request();
                        let formatted = std::str::from_utf8(
                            request
                                .headers()
                                .get("x-amz-user-agent")
                                .unwrap()
                                .as_bytes(),
                        )
                        .unwrap();
                        assert!(
                            formatted.contains("md/http##capture-request-handler"),
                            "'{}' didn't include connector metadata",
                            formatted
                        );
                    }
                    """,
                    *preludeScope,
                    "tokio" to CargoDependency.Tokio.toDevDependency().withFeature("rt").withFeature("macros").toType(),
                    "capture_request" to RuntimeType.captureRequest(rc),
                )
            }
        }
    }

    @Test
    fun `it avoids emitting repeated business metrics on retry`() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            rustCrate.integrationTest("business_metrics") {
                tokioTest("metrics_should_not_be_repeated") {
                    val rc = context.runtimeConfig
                    val moduleName = context.moduleUseName()
                    rustTemplate(
                        """
                        use $moduleName::config::{AppName, Credentials, Region, SharedCredentialsProvider};
                        use $moduleName::{Config, Client};

                        let http_client = #{StaticReplayClient}::new(vec![
                            #{ReplayEvent}::new(
                                #{HttpRequest1x}::builder()
                                    .uri("http://localhost:1234/")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                #{HttpResponse1x}::builder()
                                    .status(500)
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                            ),
                            #{ReplayEvent}::new(
                                #{HttpRequest1x}::builder()
                                    .uri("http://localhost:1234/")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                #{HttpResponse1x}::builder()
                                    .status(200)
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                            ),
                        ]);

                        let mut creds = Credentials::for_tests();
                        creds.get_property_mut_or_default::<Vec<#{AwsCredentialFeature}>>()
                            .push(#{AwsCredentialFeature}::CredentialsEnvVars);

                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(creds))
                            .retry_config(#{RetryConfig}::standard().with_max_attempts(2))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .app_name(AppName::new("test-app-name").expect("valid app name"))
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;

                        let req = http_client.actual_requests().last().unwrap();
                        let aws_ua_header = req.headers().get("x-amz-user-agent").unwrap();
                        let metrics_section = aws_ua_header
                            .split(" m/")
                            .nth(1)
                            .unwrap()
                            .split_ascii_whitespace()
                            .nth(0)
                            .unwrap();
                        assert_eq!(1, metrics_section.matches("g").count());
                        """,
                        "AwsCredentialFeature" to
                            AwsRuntimeType.awsCredentialTypes(rc)
                                .resolve("credential_feature::AwsCredentialFeature"),
                        "HttpRequest1x" to RuntimeType.HttpRequest1x,
                        "HttpResponse1x" to RuntimeType.HttpResponse1x,
                        "RetryConfig" to RuntimeType.smithyTypes(rc).resolve("retry::RetryConfig"),
                        "ReplayEvent" to RuntimeType.smithyHttpClientTestUtil(rc).resolve("test_util::ReplayEvent"),
                        "SdkBody" to RuntimeType.sdkBody(rc),
                        "StaticReplayClient" to
                            RuntimeType.smithyHttpClientTestUtil(rc)
                                .resolve("test_util::StaticReplayClient"),
                    )
                }
            }
        }
    }

    @Test
    fun `it emits business metric for RPC v2 CBOR in user agent`() {
        val model =
            """
            namespace test

            use aws.auth#sigv4
            use aws.api#service
            use smithy.protocols#rpcv2Cbor
            use smithy.rules#endpointRuleSet

            @auth([sigv4])
            @sigv4(name: "dontcare")
            @rpcv2Cbor
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {}
            })
            @service(sdkId: "dontcare")
            service TestService { version: "2023-01-01", operations: [SomeOperation] }
            structure SomeOutput { something: String }
            operation SomeOperation { output: SomeOutput }
            """.asSmithyModel()

        awsSdkIntegrationTest(model) { ctx, rustCrate ->
            rustCrate.integrationTest("business_metric_for_rpc_v2_cbor") {
                tokioTest("should_emit_metric_in_user_agent") {
                    val rc = ctx.runtimeConfig
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        use $moduleName::config::Region;
                        use $moduleName::{Client, Config};

                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["M"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }
            }
        }
    }

    @Test
    fun `it emits business metrics for retry modes`() {
        val model =
            """
            namespace test

            use aws.auth#sigv4
            use aws.api#service
            use aws.protocols#restJson1
            use smithy.rules#endpointRuleSet

            @auth([sigv4])
            @sigv4(name: "dontcare")
            @endpointRuleSet({
                "version": "1.0",
                "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
                "parameters": {}
            })
            @service(sdkId: "dontcare")
            @restJson1
            service TestService { version: "2023-01-01", operations: [SomeOperation] }

            @http(uri: "/SomeOperation", method: "GET")
            @optionalAuth
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput
            }

            @input
            structure SomeInput {}

            @output
            structure SomeOutput {}
            """.asSmithyModel()

        awsSdkIntegrationTest(model) { ctx, rustCrate ->
            rustCrate.integrationTest("retry_mode_feature_tracker") {
                val rc = ctx.runtimeConfig
                val moduleName = ctx.moduleUseName()

                rust(
                    """
                    use $moduleName::config::{Region, retry::RetryConfig};
                    use $moduleName::{Client, Config};
                    """,
                )

                tokioTest("should_emit_metric_in_user_agent_standard_mode") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .retry_config(RetryConfig::standard())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["E"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }

                tokioTest("should_emit_metric_in_user_agent_adaptive_mode") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .retry_config(RetryConfig::adaptive())
                            .with_test_defaults()
                            .build();
                        let client = Client::from_conf(config);
                        let _ = client.some_operation().send().await;
                        let expected_req = rcvr.expect_request();
                        let user_agent = expected_req
                            .headers()
                            .get("x-amz-user-agent")
                            .unwrap();
                        #{assert_ua_contains_metric_values}(user_agent, &["F"]);
                        """,
                        *preludeScope,
                        "assert_ua_contains_metric_values" to
                            AwsRuntimeType.awsRuntimeTestUtil(rc)
                                .resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "disable_interceptor" to
                            RuntimeType.smithyRuntimeApiClient(rc)
                                .resolve("client::interceptors::disable_interceptor"),
                        "UserAgentInterceptor" to
                            AwsRuntimeType.awsRuntime(rc)
                                .resolve("user_agent::UserAgentInterceptor"),
                    )
                }
            }
        }
    }
}
