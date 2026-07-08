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
 * `rpcV2CborExcludeLegacyOperationNameRoute` codegen setting.
 *
 * Per the smithy-rpc-v2 spec (https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html),
 * clients build the request URI using the verbatim operation name from the Smithy model.
 * For an operation named `getFoo`, the client POSTs to `/service/Example/operation/getFoo`.
 *
 * - When `rpcV2CborExcludeLegacyOperationNameRoute` is FALSE (default): the server router registers
 *   BOTH the spec-compliant verbatim route (e.g., `getFoo`) AND the legacy PascalCased route
 *   (e.g., `GetFoo`) when they differ, providing backward compatibility while fixing #4731.
 *   Operations already in UpperCamelCase (where names match) get a single route.
 *
 * - When `rpcV2CborExcludeLegacyOperationNameRoute` is TRUE (opt-out): the server router registers
 *   ONLY the spec-compliant verbatim route. Use this to drop the legacy PascalCased alias once
 *   clients have migrated.
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
     * Test the DEFAULT behavior (rpcV2CborExcludeLegacyOperationNameRoute = false).
     *
     * With the setting disabled (default), the router registers BOTH routes for camelCase ops:
     * - The spec-compliant verbatim route: `getFoo`
     * - The legacy PascalCased route: `GetFoo`
     *
     * This provides backward compatibility while fixing client/server interoperability.
     * Both URIs should return 200.
     */
    @Test
    fun `default behavior registers dual routes for backward compatibility`() {
        val serviceShape = model.expectShape(ShapeId.from("test#Example"))
        serverIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    service = serviceShape.id.toString(),
                    // Explicitly NOT setting rpcV2CborExcludeLegacyOperationNameRoute (defaults to false)
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

                // Test that camelCase operation name in URI returns 200 (verbatim route)
                tokioTest("default_camel_case_operation_succeeds") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With default settings, the router registers both routes.
                        // The spec-compliant verbatim URI should match.
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

                        // Default behavior: camelCase URI matches the verbatim route -> 200
                        assert!(
                            response.status().is_success(),
                            "Expected success for camelCase operation name 'getFoo' with default settings, got status {}",
                            response.status()
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                // Test that PascalCase operation name in URI also succeeds (legacy route for backward compat)
                tokioTest("default_pascal_case_uri_succeeds_for_backward_compat") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With default settings, the legacy PascalCase route is also registered.
                        // This ensures backward compatibility with clients using the old URI format.
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

                        // Default behavior: PascalCase URI matches the legacy route -> 200
                        assert!(
                            response.status().is_success(),
                            "Expected success for PascalCase URI 'GetFoo' with default settings (legacy route), got status {}",
                            response.status()
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                // Test that UpperCamelCase operations work (Smithy name == PascalCase symbol name, single route)
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
     * Test the OPT-OUT behavior (rpcV2CborExcludeLegacyOperationNameRoute = true).
     *
     * With the setting enabled, the router registers ONLY the spec-compliant verbatim route.
     * The legacy PascalCased route is excluded.
     *
     * A camelCase operation like `getFoo` is registered under `getFoo` only, so:
     * - Client URI `/service/Example/operation/getFoo` -> 200 (matches verbatim route)
     * - Client URI `/service/Example/operation/GetFoo` -> 404 (legacy route excluded)
     *
     * Use this opt-out once clients have migrated to the correct URIs.
     */
    @Test
    fun `opt-out excludes legacy route and registers only verbatim route`() {
        val serviceShape = model.expectShape(ShapeId.from("test#Example"))
        serverIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    service = serviceShape.id.toString(),
                    additionalSettings =
                        ServerAdditionalSettings.builder()
                            .rpcV2CborExcludeLegacyOperationNameRoute(true)
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

                // Test that camelCase operation name in URI succeeds (verbatim route only)
                tokioTest("optout_camel_case_operation_routes_successfully") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With opt-out setting, only the verbatim route is registered.
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
                            "Expected success for camelCase operation name 'getFoo' with opt-out setting, got status {}",
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

                // Test that PascalCase operation name in URI returns 404 (legacy route excluded)
                tokioTest("optout_pascal_case_uri_returns_404") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

                        // With opt-out setting, the legacy PascalCase route is NOT registered.
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

                        // Opt-out behavior: PascalCase URI does NOT match (legacy route excluded) -> 404
                        assert_eq!(
                            response.status(),
                            #{Http}::StatusCode::NOT_FOUND,
                            "Expected 404 for PascalCase URI 'GetFoo' with opt-out setting (legacy route excluded)"
                        );
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                // Test that UpperCamelCase operations still work with opt-out (single route, names match)
                tokioTest("optout_upper_camel_case_operation_works") {
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
                            "Expected success for UpperCamelCase operation name with opt-out setting, got status {}",
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
