/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import kotlin.io.path.readText

class RegionDecoratorTest {
    private val modelWithoutRegionParamOrSigV4AuthScheme =
        """
        namespace test

        use aws.api#service
        use aws.protocols#awsJson1_0
        use smithy.rules#endpointRuleSet

        @awsJson1_0
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
            "parameters": {}
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }
        structure SomeOutput { something: String }
        operation SomeOperation { output: SomeOutput }
        """.asSmithyModel()

    private val modelWithRegionParam =
        """
        namespace test

        use aws.api#service
        use aws.protocols#awsJson1_0
        use smithy.rules#endpointRuleSet

        @awsJson1_0
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
            "parameters": {
                "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
            }
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }
        structure SomeOutput { something: String }
        operation SomeOperation { output: SomeOutput }
        """.asSmithyModel()

    private val modelWithSigV4AuthScheme =
        """
        namespace test

        use aws.auth#sigv4
        use aws.api#service
        use aws.protocols#awsJson1_0
        use smithy.rules#endpointRuleSet

        @auth([sigv4])
        @sigv4(name: "dontcare")
        @awsJson1_0
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{ "type": "endpoint", "conditions": [], "endpoint": { "url": "https://example.com" } }],
            "parameters": {}
        })
        @service(sdkId: "dontcare")
        service TestService { version: "2023-01-01", operations: [SomeOperation] }
        structure SomeOutput { something: String }
        operation SomeOperation { output: SomeOutput }
        """.asSmithyModel()

    @Test
    fun `models without region built-in params or SigV4 should not have configurable regions`() {
        val path =
            awsSdkIntegrationTest(modelWithoutRegionParamOrSigV4AuthScheme) { _, _ ->
                // it should generate and compile successfully
            }
        val configContents = path.resolve("src/config.rs").readText()
        assertFalse(configContents.contains("fn set_region("))
    }

    @Test
    fun `models with region built-in params should have configurable regions`() {
        val path =
            awsSdkIntegrationTest(modelWithRegionParam) { _, _ ->
                // it should generate and compile successfully
            }
        val configContents = path.resolve("src/config.rs").readText()
        assertTrue(configContents.contains("fn set_region("))
    }

    @Test
    fun `models with SigV4 should have configurable regions`() {
        val path =
            awsSdkIntegrationTest(modelWithSigV4AuthScheme) { _, _ ->
                // it should generate and compile successfully
            }
        val configContents = path.resolve("src/config.rs").readText()
        assertTrue(configContents.contains("fn set_region("))
    }
}
