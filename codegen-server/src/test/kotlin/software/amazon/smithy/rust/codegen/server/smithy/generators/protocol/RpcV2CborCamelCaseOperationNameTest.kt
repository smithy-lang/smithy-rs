/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators.protocol

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests that the RPCv2 CBOR server router correctly uses the verbatim Smithy operation shape name
 * (e.g., `getFoo`) rather than the PascalCased Rust symbol name (e.g., `GetFoo`) when matching
 * incoming requests.
 *
 * Per the smithy-rpc-v2 spec (https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html),
 * clients build the request URI using the verbatim operation name from the Smithy model.
 * For an operation named `getFoo`, the client POSTs to `/service/Example/operation/getFoo`.
 * The server router must register the operation under the same verbatim name so that requests match.
 *
 * This test verifies the fix for https://github.com/smithy-lang/smithy-rs/issues/4731
 */
class RpcV2CborCamelCaseOperationNameTest {
    val model =
        """
        namespace test

        use smithy.protocols#rpcv2Cbor

        @rpcv2Cbor
        service Example {
            operations: [getFoo, UpperCaseOp],
        }

        /// An operation with a camelCase name - this is the important test case.
        /// The Smithy shape name is "getFoo", which should be preserved in the router key.
        operation getFoo {
            input:= { value: String }
            output:= { result: String }
        }

        /// An operation with UpperCamelCase name for comparison.
        operation UpperCaseOp {
            input:= { data: String }
            output:= { response: String }
        }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `camelCase operation name is preserved in router key`() {
        val serviceShape = model.expectShape(ShapeId.from("test#Example"))
        serverIntegrationTest(
            model,
            params = IntegrationTestParams(service = serviceShape.id.toString()),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    "SerdeJson" to CargoDependency.SerdeJson.toDevDependency().toType(),
                    "Ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
                    "Hyper" to RuntimeType.hyper(codegenContext.runtimeConfig),
                    "Http" to RuntimeType.http(codegenContext.runtimeConfig),
                    "Tower" to RuntimeType.Tower,
                    "HashMap" to RuntimeType.HashMap,
                    *RuntimeType.preludeScope,
                )

            rustCrate.testModule {
                rustTemplate(
                    """
                    async fn get_foo_handler(input: crate::input::GetFooInput) -> crate::output::GetFooOutput {
                        crate::output::GetFooOutput {
                            result: Some(format!("got: {}", input.value.unwrap_or_default())),
                        }
                    }

                    async fn upper_case_op_handler(input: crate::input::UpperCaseOpInput) -> crate::output::UpperCaseOpOutput {
                        crate::output::UpperCaseOpOutput {
                            response: Some(format!("upper: {}", input.data.unwrap_or_default())),
                        }
                    }

                    fn create_cbor_input(json: &str) -> Vec<u8> {
                        let value: #{SerdeJson}::Value = #{SerdeJson}::from_str(json).expect("cannot parse JSON");
                        let mut cbor_data = #{Vec}::new();
                        #{Ciborium}::ser::into_writer(&value, &mut cbor_data)
                            .expect("cannot write JSON to CBOR");
                        cbor_data
                    }
                    """,
                    *codegenScope,
                )

                // Test that camelCase operation name works with verbatim Smithy name in URI
                tokioTest("camel_case_operation_matches_verbatim_name") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // The client sends to /service/Example/operation/getFoo (verbatim camelCase name)
                        // This MUST succeed - the router should match on "getFoo", not "GetFoo"
                        let request = #{Http}::Request::builder()
                            .uri("/service/Example/operation/getFoo")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody:W})
                            .expect("Failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service, request)
                            .await
                            .expect("Failed to call service");

                        assert!(
                            response.status().is_success(),
                            "Expected success for camelCase operation name 'getFoo', got status {}",
                            response.status()
                        );

                        #{ReadBodyBytes:W}
                        let data: #{HashMap}<String, #{SerdeJson}::Value> =
                            #{Ciborium}::de::from_reader(body.as_ref()).expect("could not deserialize response");
                        let result = data.get("result")
                            .and_then(|r| r.as_str())
                            .expect("result field missing");
                        assert_eq!(result, "got: test");
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                        "ReadBodyBytes" to ServerHttpTestHelpers.httpBodyToBytes(codegenContext.runtimeConfig, "body", "response"),
                    )
                }

                // Test that using PascalCase (incorrect) returns 404
                tokioTest("pascal_case_operation_name_returns_404") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // Using PascalCase "GetFoo" instead of the correct "getFoo" should fail
                        // because the spec requires verbatim names
                        let request = #{Http}::Request::builder()
                            .uri("/service/Example/operation/GetFoo")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody:W})
                            .expect("Failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service, request)
                            .await
                            .expect("Failed to call service");

                        // PascalCase name should NOT match - expect 404
                        assert_eq!(
                            response.status(),
                            #{Http}::StatusCode::NOT_FOUND,
                            "Expected 404 for PascalCase operation name 'GetFoo' in URI"
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                // Test that UpperCamelCase operations also work correctly (both name formats match)
                tokioTest("upper_camel_case_operation_works") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"data": "hello"}"##);

                        // For operations already in UpperCamelCase, the verbatim name matches the symbol name
                        let request = #{Http}::Request::builder()
                            .uri("/service/Example/operation/UpperCaseOp")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody:W})
                            .expect("Failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service, request)
                            .await
                            .expect("Failed to call service");

                        assert!(
                            response.status().is_success(),
                            "Expected success for UpperCamelCase operation name, got status {}",
                            response.status()
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }
            }
        }
    }
}
