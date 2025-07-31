/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

/**
 * A test to ensure that authentication parameters flow properly in the case where they are set as well as the case
 * where they are not
 */
class EndpointsCredentialsTest {
    // This model has two endpoint rule paths:
    // 1. A rule that sets no authentication scheme—in this case, we should be using the default from the service
    // 2. A rule that sets a custom authentication scheme and that configures signing
    // The chosen path is controlled by static context parameters set on the operation
    private val model =
        """
        namespace aws.fooBaz

        use aws.api#service
        use aws.auth#sigv4
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use smithy.rules#staticContextParams
        use smithy.rules#clientContextParams

        @service(sdkId: "Some Value")
        @title("Test Auth Service")
        @sigv4(name: "foobaz")
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{
                          "type": "endpoint",
                          "conditions": [{"fn": "isSet", "argv": [{"ref": "AuthMode"}]}, {"fn": "stringEquals", "argv": [{"ref": "AuthMode"}, "default-auth"]}],
                          "endpoint": { "url": "https://example.com" }
                      },{
                          "type": "endpoint",
                          "conditions": [{"fn": "isSet", "argv": [{"ref": "AuthMode"}]}],
                          "endpoint": {
                            "url": "https://example.com"
                            "properties": {
                                "authSchemes": [{
                                    "name": "sigv4",
                                    "signingRegion": "region-{AuthMode}",
                                    "signingName": "name-{AuthMode}"
                                }]
                            }
                          }
                      }],
            "parameters": {
                "AuthMode": { "required": false, "type": "String" },
                "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
            }
        })
        @restJson1
        service FooBaz {
            version: "2018-03-17",
            operations: [DefaultAuth, CustomAuth]
        }

        @http(uri: "/default", method: "GET")
        @staticContextParams({ AuthMode: { value: "default-auth" } })
        operation DefaultAuth { }


        @http(uri: "/custom", method: "GET")
        @staticContextParams({ AuthMode: { value: "custom-auth" } })
        operation CustomAuth { }
        """.asSmithyModel()

    @Test
    fun `endpoint rules configure auth in default and non-default case`() {
        awsSdkIntegrationTest(model) { context, rustCrate ->
            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("endpoints_auth") {
                // assert that a rule with no default auth works properly
                tokioTest("default_auth") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(None);
                        let conf = $moduleName::Config::builder()
                            .http_client(http_client)
                            .region(#{Region}::new("us-west-2"))
                            .credentials_provider(#{Credentials}::for_tests())
                            .build();
                        let client = $moduleName::Client::from_conf(conf);
                        let _ = client.default_auth().send().await;
                        let req = rcvr.expect_request();
                        let auth_header = req.headers().get("AUTHORIZATION").unwrap();
                        assert!(auth_header.contains("/us-west-2/foobaz/aws4_request"), "{}", auth_header);
                        """,
                        "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                        "Credentials" to
                            AwsRuntimeType.awsCredentialTypesTestUtil(context.runtimeConfig)
                                .resolve("Credentials"),
                        "Region" to AwsRuntimeType.awsTypes(context.runtimeConfig).resolve("region::Region"),
                    )
                }

                // assert that auth scheme in the custom case flows through
                tokioTest("custom_auth") {
                    rustTemplate(
                        """
                        let (http_client, rcvr) = #{capture_request}(None);
                        let conf = $moduleName::Config::builder()
                            .http_client(http_client)
                            .region(#{Region}::new("us-west-2"))
                            .credentials_provider(#{Credentials}::for_tests())
                            .build();
                        let client = $moduleName::Client::from_conf(conf);
                        let _ = dbg!(client.custom_auth().send().await);
                        let req = rcvr.expect_request();
                        let auth_header = req.headers().get("AUTHORIZATION").unwrap();
                        assert!(auth_header.contains("/region-custom-auth/name-custom-auth/aws4_request"), "{}", auth_header);
                        """,
                        "capture_request" to RuntimeType.captureRequest(context.runtimeConfig),
                        "Credentials" to
                            AwsRuntimeType.awsCredentialTypesTestUtil(context.runtimeConfig)
                                .resolve("Credentials"),
                        "Region" to AwsRuntimeType.awsTypes(context.runtimeConfig).resolve("region::Region"),
                    )
                }
            }
        }
    }
}
