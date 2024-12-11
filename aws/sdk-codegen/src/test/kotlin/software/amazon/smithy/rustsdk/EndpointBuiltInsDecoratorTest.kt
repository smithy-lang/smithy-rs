/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

class EndpointBuiltInsDecoratorTest {
    private val endpointUrlModel =
        """
        namespace test

        use aws.api#service
        use aws.auth#sigv4
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use smithy.rules#staticContextParams

        @service(sdkId: "dontcare")
        @restJson1
        @sigv4(name: "dontcare")
        @auth([sigv4])
        @suppress(["RuleSetAwsBuiltIn.AWS::Auth::AccountId", "RuleSetAwsBuiltIn.AWS::Auth::AccountIdEndpointMode"])
        @endpointRuleSet({
            "version": "1.0"
            "parameters": {
                "endpoint": { "required": false, "type": "string", "builtIn": "SDK::Endpoint" },
                "region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                "accountId": { "required": false, "type": "String", "builtIn": "AWS::Auth::AccountId" },
                "accountIdEndpointMode": { "required": false, "type": "String", "builtIn": "AWS::Auth::AccountIdEndpointMode" },
            }
            "rules": [
                {
                    "type": "endpoint"
                    "conditions": [
                        {"fn": "isSet", "argv": [{"ref": "endpoint"}]},
                        {"fn": "isSet", "argv": [{"ref": "region"}]},
                        {
                            "fn": "not",
                            "argv": [
                                {
                                    "fn": "isSet",
                                    "argv": [
                                        {"ref": "accountId"}
                                    ]
                                }
                            ]
                        }
                    ],
                    "endpoint": {
                        "url": "{endpoint}"
                        "properties": {
                            "authSchemes": [{"name": "sigv4","signingRegion": "{region}", "signingName": "dontcare"}]
                        }
                    }
                },
                {
                    "type": "endpoint"
                    "conditions": [
                        {"fn": "isSet", "argv": [{"ref": "region"}]},
                    ],
                    "endpoint": {
                        "url": "https://WRONG/"
                        "properties": {
                            "authSchemes": [{"name": "sigv4", "signingRegion": "{region}", "signingName": "dontcare"}]
                        }
                    }
                }
            ]
        })
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation]
        }

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        @optionalAuth
        @staticContextParams(
            accountIdEndpointMode: {
                value: "some value"
            }
        )
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()

    @Test
    fun endpointUrlBuiltInWorksEndToEnd() {
        awsSdkIntegrationTest(endpointUrlModel) { codegenContext, rustCrate ->
            rustCrate.integrationTest("endpoint_url_built_in_works") {
                val module = codegenContext.moduleUseName()
                rustTemplate(
                    """
                    use $module::{Config, Client, config::Region};

                    ##[#{tokio}::test]
                    async fn endpoint_url_built_in_works() {
                        let http_client = #{StaticReplayClient}::new(
                            vec![#{ReplayEvent}::new(
                                http::Request::builder()
                                    .uri("https://RIGHT/SomeOperation")
                                    .body(#{SdkBody}::empty())
                                    .unwrap(),
                                http::Response::builder().status(200).body(#{SdkBody}::empty()).unwrap()
                            )],
                        );
                        let config = Config::builder()
                            .http_client(http_client.clone())
                            .region(Region::new("us-east-1"))
                            .endpoint_url("https://RIGHT")
                            .build();
                        let client = Client::from_conf(config);
                        dbg!(client.some_operation().send().await).expect("success");
                        http_client.assert_requests_match(&[]);
                    }
                    """,
                    "tokio" to CargoDependency.Tokio.toDevDependency().withFeature("rt").withFeature("macros").toType(),
                    "StaticReplayClient" to
                        CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig).toType()
                            .resolve("test_util::StaticReplayClient"),
                    "ReplayEvent" to
                        CargoDependency.smithyHttpClientTestUtil(codegenContext.runtimeConfig).toType()
                            .resolve("test_util::ReplayEvent"),
                    "SdkBody" to RuntimeType.sdkBody(codegenContext.runtimeConfig),
                )
            }
        }
    }
}
