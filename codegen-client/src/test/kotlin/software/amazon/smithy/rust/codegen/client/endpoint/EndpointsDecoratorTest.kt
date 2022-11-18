/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.endpoint

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointsDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.rulesgen.NativeSmithyEndpointsStdLib
import software.amazon.smithy.rust.codegen.client.testutil.clientIntegrationTest
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.TokioTest
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.integrationTest

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
                "conditions": [{"fn": "isSet", "argv": [{"ref":"Region"}]}],
                "type": "endpoint",
                "endpoint": {
                    "url": "https://www.{Region}.example.com"
                }
            }],
            "parameters": {
                "Bucket": { "required": false, "type": "String" },
                "Region": { "required": false, "type": "String", "builtIn": "AWS::Region" },
                "AStringParam": { "required": false, "type": "String" },
                "ABoolParam": { "required": false, "type": "Boolean" }
            }
        })
        @clientContextParams(AStringParam: {
            documentation: "string docs",
            type: "string"
        },
        aBoolParam: {
            documentation: "bool docs",
            type: "boolean"
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
            bucket: String
        }
    """.asSmithyModel()

    // NOTE: this test will fail once the endpoint starts being added directly (unless we preserve endpoint params in the
    // property bag.
    @Test
    fun `add endpoint params to the property bag`() {
        clientIntegrationTest(model, addtionalDecorators = listOf(EndpointsDecorator(), NativeSmithyEndpointsStdLib())) { clientCodegenContext, rustCrate ->
            rustCrate.integrationTest("endpoint_params_test") {
                val moduleName = clientCodegenContext.moduleUseName()
                TokioTest.render(this)
                rust(
                    """
                    async fn endpoint_params_are_set() {
                            let conf = $moduleName::Config::builder().a_string_param("hello").a_bool_param(false).build();
                            let operation = $moduleName::operation::TestOperation::builder()
                                .bucket("bucket-name").build().expect("input is valid")
                                .make_operation(&conf).await.expect("valid operation");
                            use $moduleName::endpoint::{Params};
                            use aws_smithy_http::endpoint::Result;
                            let props = operation.properties();
                            let endpoint_params = props.get::<Params>().unwrap();
                            let endpoint_result = props.get::<Result>().unwrap();
                            let endpoint = endpoint_result.as_ref().expect("endpoint resolved properly");
                            assert_eq!(
                                endpoint_params,
                                &Params::builder()
                                    .bucket("bucket-name".to_string())
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
    }
}
