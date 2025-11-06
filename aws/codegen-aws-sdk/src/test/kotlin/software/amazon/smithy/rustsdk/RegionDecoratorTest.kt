/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rustsdk

import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
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

    // V1988105516
    @Test
    fun `models with region built-in params should validate host label`() {
        awsSdkIntegrationTest(modelWithRegionParam) { ctx, rustCrate ->
            val rc = ctx.runtimeConfig
            val codegenScope =
                arrayOf(
                    *RuntimeType.preludeScope,
                    "capture_request" to RuntimeType.captureRequest(rc),
                    "Region" to AwsRuntimeType.awsTypes(rc).resolve("region::Region"),
                )

            rustCrate.integrationTest("endpoint_params_validation") {
                tokioTest("region_must_be_valid_host_label") {
                    val moduleName = ctx.moduleUseName()
                    rustTemplate(
                        """
                        let (http_client, _rx) = #{capture_request}(#{None});
                        let client_config = $moduleName::Config::builder()
                            .http_client(http_client)
                            .region(#{Region}::new("@controlled-proxy.com##"))
                            .build();

                        let client = $moduleName::Client::from_conf(client_config);

                        let err = client
                            .some_operation()
                            .send()
                            .await
                            .expect_err("error");

                        let err_str = format!("{}", $moduleName::error::DisplayErrorContext(&err));
                        dbg!(&err_str);
                        let expected = "invalid value for field: `region` - must be a valid host label";
                        assert!(err_str.contains(expected));

                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
