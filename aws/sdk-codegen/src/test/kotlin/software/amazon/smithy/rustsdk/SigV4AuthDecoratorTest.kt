/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

class SigV4AuthDecoratorTest {
    private val modelWithSigV4AuthScheme = """
        namespace test

        use aws.auth#sigv4
        use aws.api#service
        use aws.protocols#restJson1
        use smithy.rules#endpointRuleSet
        use aws.auth#unsignedPayload
        use smithy.test#httpRequestTests

        @auth([sigv4])
        @sigv4(name: "dontcare")
        @restJson1
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
            "parameters": {
                "endpoint": { "required": true, "type": "string", "builtIn": "SDK::Endpoint" },
            }
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }
        structure SomeOutput { something: String }

        structure SomeInput {
            @httpPayload
            something: Bytestream
         }

        @streaming
        blob Bytestream

        @httpRequestTests([{
            id: "unsignedPayload",
            protocol: restJson1,
            method: "POST",
            uri: "/",
            params: {
                something: "hello"
            },
            headers: {
                "x-amz-content-sha256": "UNSIGNED-PAYLOAD",
            },
        }])
        @unsignedPayload
        @http(uri: "/", method: "POST")
        operation SomeOperation { input: SomeInput, output: SomeOutput }
    """.asSmithyModel()

    @Test
    fun unsignedPayloadSetsCorrectHeader() {
        awsSdkIntegrationTest(modelWithSigV4AuthScheme) { _, _ -> }
    }
}
