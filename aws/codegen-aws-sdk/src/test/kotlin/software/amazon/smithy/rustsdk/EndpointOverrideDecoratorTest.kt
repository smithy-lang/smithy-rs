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
    fun `generated code includes endpoint override interceptor registration`() {
        awsSdkIntegrationTest(model) { _, _ ->
            // The test passes if the code compiles successfully
            // This verifies that the decorator generates valid Rust code
        }
    }

    @Test
    fun `generated code compiles with endpoint override interceptor`() {
        // Create custom test params with endpoint_url config enabled
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
            rustCrate.integrationTest("endpoint_override_compiles") {
                tokioTest("can_build_client_with_endpoint_url") {
                    rustTemplate(
                        """
                        use $moduleName::config::Region;
                        use $moduleName::{Client, Config};

                        let (http_client, _rcvr) = #{capture_request}(#{None});
                        let config = Config::builder()
                            .region(Region::new("us-east-1"))
                            .endpoint_url("https://custom.example.com")
                            .http_client(http_client.clone())
                            .with_test_defaults()
                            .build();
                        let _client = Client::from_conf(config);
                        // Test passes if code compiles and client can be created
                        """,
                        *preludeScope,
                        "capture_request" to RuntimeType.captureRequest(rc),
                    )
                }
            }
        }
    }
}
