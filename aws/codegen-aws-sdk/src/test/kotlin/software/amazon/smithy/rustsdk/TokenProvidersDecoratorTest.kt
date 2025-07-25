/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest

class TokenProvidersDecoratorTest {
    val model =
        """
        namespace test

        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet

        @service(sdkId: "dontcare")
        @restJson1
        @httpBearerAuth
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

        structure SomeOutput {
            someAttribute: Long,
            someVal: String
        }

        @http(uri: "/SomeOperation", method: "GET")
        operation SomeOperation {
            output: SomeOutput
        }
        """.asSmithyModel()

    @Test
    fun `it adds token provider configuration`() {
        awsSdkIntegrationTest(model) { ctx, rustCrate ->
            rustCrate.integrationTest("token_providers") {
                tokioTest("configuring_credentials_provider_at_operation_level_should_work") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        // this should compile
                        let _ = $moduleName::Config::builder()
                            .token_provider(#{TestToken}::for_tests())
                            .build();

                        // this should also compile
                        $moduleName::Config::builder()
                            .set_token_provider(Some(#{SharedTokenProvider}::new(#{TestToken}::for_tests())));
                        """,
                        "TestToken" to AwsRuntimeType.awsCredentialTypesTestUtil(ctx.runtimeConfig).resolve("Token"),
                        "SharedTokenProvider" to
                            AwsRuntimeType.awsCredentialTypes(ctx.runtimeConfig)
                                .resolve("provider::token::SharedTokenProvider"),
                    )
                }
            }
        }
    }
}
