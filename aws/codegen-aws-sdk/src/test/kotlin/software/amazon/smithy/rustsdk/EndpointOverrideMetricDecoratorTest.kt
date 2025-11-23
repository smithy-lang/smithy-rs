/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

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

        awsSdkIntegrationTest(model, testParams) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("endpoint_override_via_sdk_config") {
                rustTemplate(
                    """
                    use $moduleName::config::Region;
                    use $moduleName::Client;
                    use #{capture_request};

                    ##[#{tokio}::test]
                    async fn metric_tracked_when_endpoint_set_via_sdk_config() {
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

                        // Verify metric 'N' is present
                        let user_agent = std::str::from_utf8(
                            request
                                .headers()
                                .get("x-amz-user-agent")
                                .expect("x-amz-user-agent header missing")
                                .as_bytes(),
                        )
                        .expect("valid utf8");

                        let has_metric = user_agent
                            .split_whitespace()
                            .any(|part| part.starts_with("m/") && part.contains("N"));

                        assert!(
                            has_metric,
                            "Expected metric 'N' in user agent, got: {}",
                            user_agent
                        );
                    }
                    """,
                    *preludeScope,
                    "capture_request" to RuntimeType.captureRequest(rc),
                    "SdkConfig" to AwsRuntimeType.awsTypes(rc).resolve("sdk_config::SdkConfig"),
                    "tokio" to CargoDependency.Tokio.toType(),
                )
            }
        }
    }

    @Test
    fun `no endpoint override metric when endpoint not set`() {
        val testParams = awsIntegrationTestParams()

        awsSdkIntegrationTest(model, testParams) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("no_endpoint_override") {
                rustTemplate(
                    """
                    use $moduleName::config::{Credentials, Region, SharedCredentialsProvider};
                    use $moduleName::{Config, Client};
                    use #{capture_request};

                    ##[#{tokio}::test]
                    async fn no_metric_when_endpoint_not_overridden() {
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
                        let user_agent = std::str::from_utf8(
                            request
                                .headers()
                                .get("x-amz-user-agent")
                                .map(|v| v.as_bytes())
                                .unwrap_or(b""),
                        )
                        .unwrap_or("");

                        let has_metric = user_agent
                            .split_whitespace()
                            .any(|part| part.starts_with("m/") && part.contains("N"));

                        assert!(
                            !has_metric,
                            "Did not expect metric 'N' when endpoint not overridden, got: {}",
                            user_agent
                        );
                    }
                    """,
                    *preludeScope,
                    "capture_request" to RuntimeType.captureRequest(rc),
                    "tokio" to CargoDependency.Tokio.toType(),
                )
            }
        }
    }
}
