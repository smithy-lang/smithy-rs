/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest

class TestPromoteEndpointBuiltin {
    private val model = """
        namespace aws.testEndpointBuiltIn

        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use smithy.rules#staticContextParams
        use smithy.rules#clientContextParams

        @service(sdkId: "Some Value")
        @title("Test Auth Service")
        @endpointRuleSet({
            parameters: {
                CustomEndpoint: { "type": "string", "builtIn": "SDK::Endpoint", "documentation": "Sdk endpoint" }
            },
            version: "1.0",
            rules: [
                {
                    "type": "endpoint",
                    "conditions": [],
                    "endpoint": {
                      "url": "https://foo.com"
                    }
                }
            ]
        })
        @restJson1
        service FooBaz {
            version: "2018-03-17",
            operations: [NoOp]
        }

        @http(uri: "/blah", method: "GET")
        operation NoOp {}
    """.asSmithyModel()

    @Test
    fun promoteStringBuiltIn() {
        awsSdkIntegrationTest(
            model.promoteBuiltInToContextParam(
                ShapeId.from("aws.testEndpointBuiltIn#FooBaz"),
                Builtins.SDK_ENDPOINT,
            ),
        ) { context, rustCrate ->

            val moduleName = context.moduleUseName()
            rustCrate.integrationTest("builtin_as_string") {
                // assert that a rule with no default auth works properly
                unitTest("set_endpoint") {
                    rustTemplate(
                        """
                        let _ = $moduleName::Config::builder().custom_endpoint("asdf").build();
                        """,
                    )
                }
            }
        }
    }
}
