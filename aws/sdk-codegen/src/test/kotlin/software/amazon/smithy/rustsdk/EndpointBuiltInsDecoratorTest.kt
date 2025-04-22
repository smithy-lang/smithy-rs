/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq

class EndpointBuiltInsDecoratorTest {
    @Test
    fun endpointUrlBuiltInWorksEndToEnd() {
        val endpointUrlModel =
            """
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
                "version": "1.0"
                "parameters": {
                    "endpoint": { "required": false, "type": "string", "builtIn": "SDK::Endpoint" },
                    "region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                }
                "rules": [
                    {
                        "type": "endpoint"
                        "conditions": [
                            {"fn": "isSet", "argv": [{"ref": "endpoint"}]},
                            {"fn": "isSet", "argv": [{"ref": "region"}]},
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
            operation SomeOperation {
                output: SomeOutput
            }
            """.asSmithyModel(smithyVersion = "2.0")

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

    private fun modelWithAccountId(authAnnotation: String? = null) =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet

        @service(sdkId: "dontcare")
        @restJson1
        """ + (authAnnotation.orEmpty()) +
            """
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
                            "url": "https://ACCOUNT-ID-NOT-SET/"
                        }
                    },
                    {
                        "type": "endpoint"
                        "conditions": [
                            {"fn": "isSet", "argv": [{"ref": "region"}]},
                            {
                                "fn": "isSet",
                                "argv": [
                                    {"ref": "accountId"}
                                ]
                            }
                        ],
                        "endpoint": {
                            "url": "https://ACCOUNT-ID-SET/"
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
            operation SomeOperation {
                output: SomeOutput
            }
            """

    @Test
    fun accountIdBuiltInParam() {
        fun runTest(
            expectedUrl: String,
            credentialsProvider: (CodegenContext) -> Writable,
            authAnnotation: String? = null,
        ) {
            awsSdkIntegrationTest(modelWithAccountId(authAnnotation).asSmithyModel(smithyVersion = "2.0")) { ctx, rustCrate ->
                rustCrate.integrationTest("account_id_built_in_param") {
                    tokioTest("should_work") {
                        val module = ctx.moduleUseName()
                        rustTemplate(
                            """
                            use $module::{Config, Client, config::Region};

                            let (conn, req) = #{capture_request}(None);
                            let config = Config::builder()
                                #{creds_provider}
                                .http_client(conn)
                                .region(Region::new("us-east-1"))
                                .build();
                            let client = Client::from_conf(config);
                            let _ = dbg!(client.some_operation().send().await);
                            let req = req.expect_request();
                            assert_eq!(${expectedUrl.dq()}, req.uri());
                            """,
                            "capture_request" to RuntimeType.captureRequest(ctx.runtimeConfig),
                            "creds_provider" to credentialsProvider(ctx),
                        )
                    }
                }
            }
        }

        // The model uses no authentication (`NoAuthScheme`), causing  `NoAuthIdentityResolver` to resolve a
        // `NoAuthIdentity`. Since `NoAuthIdentity` lacks an account ID, this test is intended
        // to exercise an endpoint rule whose condition expects the account ID to be unset.
        runTest("https://ACCOUNT-ID-NOT-SET/SomeOperation", { _ -> writable {} })

        // The model uses SigV4 authentication, with the credentials provider returning credentials
        // containing an account ID. This should exercise an endpoint rule whose condition expects the account ID to
        // be set.
        runTest(
            "https://ACCOUNT-ID-SET/SomeOperation",
            { ctx ->
                writable {
                    rustTemplate(
                        """
                        .credentials_provider(
                            #{SharedCredentialsProvider}::new(
                                #{CredentialsBuilder}::for_tests()
                                    .account_id("123456789012")
                                    .build()
                            )
                        )
                        """,
                        "SharedCredentialsProvider" to
                            AwsRuntimeType.awsCredentialTypes(ctx.runtimeConfig)
                                .resolve("provider::SharedCredentialsProvider"),
                        "CredentialsBuilder" to
                            AwsRuntimeType.awsCredentialTypesTestUtil(ctx.runtimeConfig)
                                .resolve("CredentialsBuilder"),
                    )
                }
            },
            "@aws.auth#sigv4(name: \"donotcare\")\n",
        )
    }
}
