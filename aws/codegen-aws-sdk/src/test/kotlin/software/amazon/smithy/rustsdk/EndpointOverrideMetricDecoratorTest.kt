/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class EndpointOverrideMetricDecoratorTest {
    companion object {
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
                "rules": [
                    {
                        "type": "endpoint",
                        "conditions": [
                            { "fn": "isSet", "argv": [{ "ref": "Endpoint" }] }
                        ],
                        "endpoint": { "url": { "ref": "Endpoint" } }
                    },
                    {
                        "type": "endpoint",
                        "conditions": [],
                        "endpoint": { "url": "https://example.com" }
                    }
                ],
                "parameters": {
                    "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                    "Endpoint": { "required": false, "type": "String", "builtIn": "SDK::Endpoint" }
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
    fun `decorator is registered in AwsCodegenDecorator list`() {
        val decoratorNames = DECORATORS.map { it.name }
        assert(decoratorNames.contains("EndpointOverrideMetric")) {
            "EndpointOverrideMetricDecorator should be registered in DECORATORS list. Found: $decoratorNames"
        }
    }

    @Test
    fun `endpoint override metric appears when set via SdkConfig`() {
        val testParams = awsIntegrationTestParams()

        awsSdkIntegrationTest(
            model,
            testParams,
            environment = mapOf("RUSTUP_TOOLCHAIN" to "1.88.0"),
        ) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()

            // Enable test-util feature for aws-runtime
            rustCrate.mergeFeature(Feature("test-util", true, listOf("aws-runtime/test-util")))

            rustCrate.integrationTest("endpoint_override_via_sdk_config") {
                tokioTest("metric_tracked_when_endpoint_set_via_sdk_config") {
                    rustTemplate(
                        """
                        use $moduleName::config::Region;
                        use $moduleName::Client;
                        use #{capture_request};
                        use #{assert_ua_contains_metric_values};
                        
                        let (http_client, rcvr) = capture_request(None);
                        
                        // Create SdkConfig with endpoint URL
                        let sdk_config = #{SdkConfig}::builder()
                            .region(Region::new("us-east-1"))
                            .endpoint_url("https://sdk-custom.example.com")
                            .http_client(http_client.clone())
                            .build();
                        
                        // Create client from SdkConfig
                        let client = Client::new(&sdk_config);

                        // Make a request
                        let _ = client.some_operation().send().await;

                        // Verify the request
                        let request = rcvr.expect_request();

                        // Verify endpoint was overridden
                        let uri = request.uri().to_string();
                        assert!(
                            uri.starts_with("https://sdk-custom.example.com"),
                            "Expected SDK custom endpoint, got: {}",
                            uri
                        );

                        // Verify metric 'N' is present in x-amz-user-agent header
                        let user_agent = request
                            .headers()
                            .get("x-amz-user-agent")
                            .expect("x-amz-user-agent header missing");
                        
                        assert_ua_contains_metric_values(user_agent, &["N"]);
                        """,
                        *preludeScope,
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "assert_ua_contains_metric_values" to AwsRuntimeType.awsRuntime(rc).resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                        "SdkConfig" to AwsRuntimeType.awsTypes(rc).resolve("sdk_config::SdkConfig"),
                    )
                }
            }
        }
    }

    @Test
    fun `no endpoint override metric when endpoint not set`() {
        val testParams = awsIntegrationTestParams()

        awsSdkIntegrationTest(
            model,
            testParams,
            environment = mapOf("RUSTUP_TOOLCHAIN" to "1.88.0"),
        ) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()

            // Enable test-util feature for aws-runtime
            rustCrate.mergeFeature(Feature("test-util", true, listOf("aws-runtime/test-util")))

            rustCrate.integrationTest("no_endpoint_override") {
                tokioTest("no_metric_when_endpoint_not_overridden") {
                    rustTemplate(
                        """
                        use $moduleName::config::{Credentials, Region, SharedCredentialsProvider};
                        use $moduleName::{Config, Client};
                        use #{capture_request};
                        
                        let (http_client, rcvr) = capture_request(None);
                        
                        // Create config WITHOUT endpoint override
                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .build();
                        let client = Client::from_conf(config);

                        // Make a request
                        let _ = client.some_operation().send().await;

                        // Verify the request
                        let request = rcvr.expect_request();

                        // Verify default endpoint was used
                        let uri = request.uri().to_string();
                        assert!(
                            uri.starts_with("https://example.com"),
                            "Expected default endpoint, got: {}",
                            uri
                        );

                        // Verify metric 'N' is NOT present
                        let user_agent = request
                            .headers()
                            .get("x-amz-user-agent")
                            .expect("x-amz-user-agent header should be present");

                        assert!(
                            !user_agent.contains("m/N"),
                            "Metric 'N' should NOT be present when endpoint not overridden"
                        );
                        """,
                        *preludeScope,
                        "capture_request" to RuntimeType.captureRequest(rc),
                    )
                }

                // Add a should_panic test to verify assert_ua_contains_metric_values panics when metric is not present
                rust("##[should_panic(expected = \"metric values\")]")
                tokioTest("assert_panics_when_metric_not_present") {
                    rustTemplate(
                        """
                        use $moduleName::config::{Credentials, Region, SharedCredentialsProvider};
                        use $moduleName::{Config, Client};
                        use #{capture_request};
                        use #{assert_ua_contains_metric_values};

                        let (http_client, rcvr) = capture_request(None);

                        let config = Config::builder()
                            .credentials_provider(SharedCredentialsProvider::new(Credentials::for_tests()))
                            .region(Region::new("us-east-1"))
                            .http_client(http_client.clone())
                            .build();
                        let client = Client::from_conf(config);

                        let _ = client.some_operation().send().await;
                        let request = rcvr.expect_request();
                        let user_agent = request.headers().get("x-amz-user-agent").unwrap();

                        // This should panic because 'N' is not present
                        assert_ua_contains_metric_values(user_agent, &["N"]);
                        """,
                        *preludeScope,
                        "capture_request" to RuntimeType.captureRequest(rc),
                        "assert_ua_contains_metric_values" to AwsRuntimeType.awsRuntime(rc).resolve("user_agent::test_util::assert_ua_contains_metric_values"),
                    )
                }
            }
        }
    }
}
