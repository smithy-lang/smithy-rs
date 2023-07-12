/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.endpoint

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest
import software.amazon.smithy.rust.codegen.core.testutil.runWithWarnings
import software.amazon.smithy.rust.codegen.core.util.CommandError

/**
 * End-to-end test of endpoint resolvers, attaching a real resolver to a fully generated service
 */
class EndpointsDecoratorTest {
    val model = """
        namespace test

        use smithy.rules#endpointRuleSet
        use smithy.rules#endpointTests

        use smithy.rules#clientContextParams
        use smithy.rules#staticContextParams
        use smithy.rules#contextParam
        use aws.protocols#awsJson1_1

        @awsJson1_1
        @endpointRuleSet({
            "version": "1.0",
            "rules": [{
                "conditions": [
                    {"fn": "isSet", "argv": [{"ref":"Region"}]},
                    {"fn": "isSet", "argv": [{"ref":"ABoolParam"}]},
                    {"fn": "booleanEquals", "argv": [{"ref": "ABoolParam"}, false]}
                ],
                "type": "endpoint",
                "endpoint": {
                    "url": "https://www.{Region}.example.com",
                    "properties": {
                        "first-properties": {
                            "z-first": "zazz",
                            "y-second": "bar",
                            "x-third": "baz"
                        },
                        "second-properties": [1,2,3]
                    }
                }
            }],
            "parameters": {
                "Bucket": { "required": false, "type": "String" },
                "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                "BuiltInWithDefault": { "required": true, "type": "String", "builtIn": "AWS::DefaultBuiltIn", "default": "some-default" },
                "BoolBuiltInWithDefault": { "required": true, "type": "Boolean", "builtIn": "AWS::FooBar", "default": true },
                "AStringParam": { "required": false, "type": "String" },
                "ABoolParam": { "required": false, "type": "Boolean" }
            }
        })
        @clientContextParams(
            AStringParam: {
                documentation: "string docs",
                type: "string"
            },
            aBoolParam: {
                documentation: "bool docs",
                type: "boolean"
            }
        )
        @endpointTests({
          "version": "1.0",
          "testCases": [
            {
              "documentation": "uriEncode when the string has nothing to encode returns the input",
              "params": {
                "Region": "test-region"
              },
              "operationInputs": [
                { "operationName": "TestOperation", "operationParams": { "nested": { "field": "test" } } }
              ],
              "expect": {
                "endpoint": {
                    "url": "https://failingtest.com"
                    "properties": {
                        "first-properties": {
                            "a-first": "zazz",
                            "b-second": "bar",
                            "c-third": "baz"
                        },
                        "second-properties": [1,2,3]
                    }
                }
              }
            }
         ]
        })
        service TestService {
            operations: [TestOperation]
        }

        @staticContextParams(Region: { value: "us-east-2" })
        operation TestOperation {
            input: TestOperationInput
        }

        structure TestOperationInput {
            @contextParam(name: "Bucket")
            bucket: String,
            nested: NestedStructure
        }

        structure NestedStructure {
            field: String
        }
    """.asSmithyModel()

    @Test
    fun `set an endpoint in the property bag`() {
        val testDir = clientIntegrationTest(
            model,
            // Just run integration tests.
            IntegrationTestParams(command = { "cargo test --test *".runWithWarnings(it) }),
        ) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("endpoint_params_test") {
                val moduleName = clientCodegenContext.moduleUseName()
                Attribute.TokioTest.render(this)
                rust(
                    """
                    async fn endpoint_params_are_set() {
                            let conf = $moduleName::Config::builder().a_string_param("hello").a_bool_param(false).build();
                            let operation = $moduleName::operation::test_operation::TestOperationInput::builder()
                                .bucket("bucket-name").build().expect("input is valid")
                                .make_operation(&conf).await.expect("valid operation");
                            use $moduleName::endpoint::{Params};
                            use aws_smithy_http::endpoint::Result;
                            let props = operation.properties();
                            let endpoint_result = dbg!(props.get::<Result>().expect("endpoint result in the bag"));
                            let endpoint_params = props.get::<Params>().expect("endpoint params in the bag");
                            let endpoint = endpoint_result.as_ref().expect("endpoint resolved properly");
                            assert_eq!(
                                endpoint_params,
                                &Params::builder()
                                    .bucket("bucket-name".to_string())
                                    .built_in_with_default("some-default")
                                    .bool_built_in_with_default(true)
                                    .a_bool_param(false)
                                    .a_string_param("hello".to_string())
                                    .region("us-east-2".to_string())
                                    .build().unwrap()
                            );

                            assert_eq!(endpoint.url(), "https://www.us-east-2.example.com");
                    }
                    """,
                )
            }
        }
        // the model has an intentionally failing test—ensure it fails
        val failure = shouldThrow<CommandError> { "cargo test".runWithWarnings(testDir) }
        failure.output shouldContain "endpoint::test::test_1"
        failure.output shouldContain "https://failingtest.com"
        "cargo clippy".runWithWarnings(testDir)
    }
}
