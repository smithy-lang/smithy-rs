/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class EndpointOverrideDecoratorTest {
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
    fun `decorator is registered in AwsCodegenDecorator list`() {
        // Verify that EndpointOverrideDecorator is in the DECORATORS list
        val decoratorNames = DECORATORS.map { it.name }
        assert(decoratorNames.contains("EndpointOverride")) {
            "EndpointOverrideDecorator should be registered in DECORATORS list"
        }
    }

    @Test
    fun `endpoint override interceptor adds business metric to user agent`() {
        val testParams =
            awsIntegrationTestParams().copy(
                additionalSettings =
                    awsIntegrationTestParams().additionalSettings.toBuilder()
                        .withMember(
                            "codegen",
                            software.amazon.smithy.model.node.ObjectNode.builder()
                                .withMember("includeFluentClient", false)
                                .withMember("includeEndpointUrlConfig", true)
                                .build(),
                        )
                        .build(),
            )

        awsSdkIntegrationTest(model, testParams) { context, rustCrate ->
            val rc = context.runtimeConfig
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("endpoint_override_functional") {
                tokioTest("interceptor_adds_metric_when_endpoint_overridden") {
                    rustTemplate(
                        """
                        use $moduleName::config::Region;
                        use $moduleName::{Client, Config};

                        let (http_client, rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .endpoint_url("https://custom.example.com")
                            .http_client(http_client.clone())
                            .build();
                        let client = Client::from_conf(config);

                        // CRITICAL: Actually make a request
                        let _ = client.some_operation().send().await;

                        // Capture and verify the request
                        let request = rcvr.expect_request();

                        // Verify endpoint was overridden
                        let uri = request.uri().to_string();
                        assert!(
                            uri.starts_with("https://custom.example.com"),
                            "Expected custom endpoint, got: {}",
                            uri
                        );

                        // Verify x-amz-user-agent contains business metric 'N' (endpoint override)
                        // The metric appears in the business-metrics section as "m/..." with comma-separated IDs
                        let x_amz_user_agent = request.headers()
                            .get("x-amz-user-agent")
                            .expect("x-amz-user-agent header missing");

                        // Extract the business metrics section (starts with "m/")
                        let has_endpoint_override_metric = x_amz_user_agent
                            .split_whitespace()
                            .find(|part| part.starts_with("m/"))
                            .map(|metrics| {
                                // Check if 'N' appears as a metric ID (either alone or in a comma-separated list)
                                metrics.strip_prefix("m/")
                                    .map(|ids| ids.split(',').any(|id| id == "N"))
                                    .unwrap_or(false)
                            })
                            .unwrap_or(false);

                        assert!(
                            has_endpoint_override_metric,
                            "Expected metric ID 'N' (endpoint override) in x-amz-user-agent business metrics, got: {}",
                            x_amz_user_agent
                        );
                        """,
                        *preludeScope,
                        "capture_request" to RuntimeType.captureRequest(rc),
                    )
                }
            }
        }
    }
}
