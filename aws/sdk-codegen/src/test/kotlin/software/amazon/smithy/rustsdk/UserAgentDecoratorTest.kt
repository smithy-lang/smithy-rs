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
}
