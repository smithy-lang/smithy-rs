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
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests that the RPCv2 CBOR server router correctly handles operation name casing based on the
 * `rpcV2CborUseVerbatimOperationName` codegen setting.
 *
 * Per the smithy-rpc-v2 spec (https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html),
 * clients build the request URI using the verbatim operation name from the Smithy model.
 * For an operation named `getFoo`, the client POSTs to `/service/Example/operation/getFoo`.
 *
 * - When `rpcV2CborUseVerbatimOperationName` is FALSE (default): the server router uses PascalCased
 *   Rust symbol names (e.g., `GetFoo`), preserving historical behavior for backwards compatibility.
 *   This means camelCase operations will 404 because the client sends `getFoo` but the server
 *   expects `GetFoo`.
 *
 * - When `rpcV2CborUseVerbatimOperationName` is TRUE (opt-in): the server router uses verbatim
 *   Smithy operation names, matching the generated client and the spec. This fixes
 *   https://github.com/smithy-lang/smithy-rs/issues/4731
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
        /// The Smithy shape name is "getFoo", which should be preserved in the router key
        /// when the verbatim operation name setting is enabled.
        operation getFoo {
            input:= { value: String }
            output:= { result: String }
        }

        /// An operation with UpperCamelCase name for comparison.
        /// This works correctly in both modes since PascalCase matches the Smithy name.
        operation UpperCaseOp {
            input:= { data: String }
            output:= { response: String }
        }
        """.asSmithyModel(smithyVersion = "2")

    /**
     * Test the DEFAULT behavior (rpcV2CborUseVerbatimOperationName = false).
     *
     * With the setting disabled (default), the router uses PascalCased symbol names.
     * A camelCase operation like `getFoo` is registered under `GetFoo`, so:
     * - Client URI `/service/Example/operation/getFoo` -> 404 (no match)
     * - Client URI `/service/Example/operation/GetFoo` -> 200 (matches PascalCase key)
     *
     * This preserves the historical behavior for backwards compatibility.
     */
    @Test
    fun `default behavior preserves PascalCase router keys`() {
        val serviceShape = model.expectShape(ShapeId.from("test#Example"))
        serverIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    service = serviceShape.id.toString(),
                    // Explicitly NOT setting rpcV2CborUseVerbatimOperationName (defaults to false)
                ),
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

                // Test that camelCase operation name in URI returns 404 (default behavior)
                tokioTest("default_camel_case_operation_returns_404") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With default settings, the router is keyed on PascalCase "GetFoo".
                        // The client sends camelCase "getFoo", which does NOT match.
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

                        // Default behavior: camelCase URI does NOT match PascalCase router key -> 404
                        assert_eq!(
                            response.status(),
                            #{Http}::StatusCode::NOT_FOUND,
                            "Expected 404 for camelCase operation name 'getFoo' with default settings (router keyed on 'GetFoo')"
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                // Test that PascalCase operation name in URI succeeds (matches the router key)
                tokioTest("default_pascal_case_uri_succeeds") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With default settings, using PascalCase "GetFoo" matches the router key.
                        // Note: This is NOT spec-compliant client behavior, but confirms the router key is PascalCase.
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

                        assert!(
                            response.status().is_success(),
                            "Expected success for PascalCase URI 'GetFoo' with default settings, got status {}",
                            response.status()
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                // Test that UpperCamelCase operations work (Smithy name == PascalCase symbol name)
                tokioTest("default_upper_camel_case_operation_works") {
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

    /**
     * Test the OPT-IN behavior (rpcV2CborUseVerbatimOperationName = true).
     *
     * With the setting enabled, the router uses verbatim Smithy operation names.
     * A camelCase operation like `getFoo` is registered under `getFoo`, so:
     * - Client URI `/service/Example/operation/getFoo` -> 200 (matches verbatim key)
     * - Client URI `/service/Example/operation/GetFoo` -> 404 (no match)
     *
     * This is the spec-compliant behavior that fixes #4731.
     */
    @Test
    fun `opt-in enables verbatim operation names in router`() {
        val serviceShape = model.expectShape(ShapeId.from("test#Example"))
        serverIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    service = serviceShape.id.toString(),
                    additionalSettings =
                        ServerAdditionalSettings.builder()
                            .rpcV2CborUseVerbatimOperationName(true)
                            .toObjectNode(),
                ),
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

                // Test that camelCase operation name in URI succeeds (opt-in behavior)
                tokioTest("optin_camel_case_operation_routes_successfully") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With opt-in setting, the router is keyed on verbatim "getFoo".
                        // The client sends camelCase "getFoo", which matches.
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
                            "Expected success for camelCase operation name 'getFoo' with opt-in setting, got status {}",
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

                // Test that PascalCase operation name in URI returns 404 (opt-in behavior)
                tokioTest("optin_pascal_case_uri_returns_404") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With opt-in setting, using PascalCase "GetFoo" does NOT match the verbatim key "getFoo".
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

                        // Opt-in behavior: PascalCase URI does NOT match verbatim router key -> 404
                        assert_eq!(
                            response.status(),
                            #{Http}::StatusCode::NOT_FOUND,
                            "Expected 404 for PascalCase URI 'GetFoo' with opt-in setting (router keyed on 'getFoo')"
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                // Test that UpperCamelCase operations still work with opt-in
                tokioTest("optin_upper_camel_case_operation_works") {
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
                            "Expected success for UpperCamelCase operation name with opt-in setting, got status {}",
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
