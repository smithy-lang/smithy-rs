/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class SmokeTestsDecoratorTest {
    companion object {
        // Can't use the dollar sign in a multiline string with doing it like this.
        private const val PREFIX = "\$version: \"2\""
        val model =
            """
            $PREFIX
            namespace test

            use aws.api#service
            use smithy.test#smokeTests
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

            @smokeTests([
                {
                    id: "SomeOperationSuccess",
                    params: {}
                    vendorParams: {
                        region: "us-west-2"
                    }
                    expect: { success: {} }
                }
                {
                    id: "SomeOperationFailure",
                    params: {}
                    vendorParams: {
                        region: "us-west-2"
                    }
                    expect: { failure: {} }
                }
                {
                    id: "SomeOperationFailureExplicitShape",
                    params: {}
                    vendorParams: {
                        region: "us-west-2"
                    }
                    expect: {
                        failure: { errorId: FooException }
                    }
                }
            ])
            @http(uri: "/SomeOperation", method: "POST")
            @optionalAuth
            operation SomeOperation {
                input: SomeInput,
                output: SomeOutput,
                errors: [FooException]
            }

            @input
            structure SomeInput {}

            @output
            structure SomeOutput {}

            @error("server")
            structure FooException { }
            """.asSmithyModel()
    }

    @Test
    fun smokeTestSdkCodegen() {
        awsSdkIntegrationTest(model) { _, _ ->
            // It should compile. We can't run the tests
            // because they don't target a real service.
        }
    }
}
