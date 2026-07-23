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
 * `rpcV2CborAddCapitalizedRoute` codegen setting.
 *
 * Per the smithy-rpc-v2 spec (https://smithy.io/2.0/additional-specs/protocols/smithy-rpc-v2.html),
 * clients build the request URI using the verbatim operation name from the Smithy model.
 * For an operation named `getFoo`, the client POSTs to `/service/Example/operation/getFoo`.
 *
 * - When `rpcV2CborAddCapitalizedRoute` is FALSE (default): the server router registers
 *   ONLY the spec-compliant verbatim route (e.g., `getFoo`).
 *
 * - When `rpcV2CborAddCapitalizedRoute` is TRUE (opt-in): the server router also registers
 *   the legacy capitalized route (e.g., `GetFoo`) for backward compatibility.
 */
class RpcV2CborCamelCaseOperationNameTest {
    val model =
        """
        namespace test

        use smithy.protocols#rpcv2Cbor

        @rpcv2Cbor
        service Example {
            operations: [getFoo, UpperCaseOp, myAPIOp],
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

        /// An operation with an acronym (API) to test PascalCase transformation.
        /// Smithy shape name "myAPIOp" becomes Rust symbol "MyAPIOp".
        operation myAPIOp {
            input:= { param: String }
            output:= { out: String }
        }
        """.asSmithyModel(smithyVersion = "2")

    val collidingOperationNamesModel =
        """
        namespace test

        use smithy.protocols#rpcv2Cbor

        @rpcv2Cbor
        service ExampleCollision {
            operations: [getFoo, getFoo_1],
        }

        operation getFoo {
            input:= { value: String }
            output:= { result: String }
        }

        operation getFoo_1 {
            input:= { value: String }
            output:= { result: String }
        }
        """.asSmithyModel(smithyVersion = "2")

    /**
     * Test the DEFAULT behavior (`rpcV2CborAddCapitalizedRoute = false`).
     *
     * With the setting disabled (default), the router registers only the spec-compliant
     * verbatim route for camelCase operations.
     */
    @Test
    fun `default behavior registers only the verbatim route`() {
        val serviceShape = model.expectShape(ShapeId.from("test#Example"))
        serverIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    service = serviceShape.id.toString(),
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

                    async fn my_api_op_handler(input: crate::input::MyApiOpInput) -> crate::output::MyApiOpOutput {
                        crate::output::MyApiOpOutput {
                            out: Some(format!("api: {}", input.param.unwrap_or_default())),
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

                tokioTest("default_camel_case_verbatim_route_succeeds") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

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

                        assert!(response.status().is_success());

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

                tokioTest("default_capitalized_uri_returns_404") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

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

                        assert_eq!(response.status(), #{Http}::StatusCode::NOT_FOUND);
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                tokioTest("default_already_capitalized_operation_works") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"data": "hello"}"##);

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

                        assert!(response.status().is_success());
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                tokioTest("default_acronym_verbatim_route_succeeds") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"param": "test"}"##);

                        let request = #{Http}::Request::builder()
                            .uri("/service/Example/operation/myAPIOp")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody:W})
                            .expect("Failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service, request)
                            .await
                            .expect("Failed to call service");

                        assert!(response.status().is_success());
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                tokioTest("default_acronym_capitalized_uri_returns_404") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"param": "test"}"##);

                        let request = #{Http}::Request::builder()
                            .uri("/service/Example/operation/MyAPIOp")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody:W})
                            .expect("Failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service, request)
                            .await
                            .expect("Failed to call service");

                        assert_eq!(response.status(), #{Http}::StatusCode::NOT_FOUND);
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }
            }
        }
    }

    /**
     * Test the OPT-IN behavior (`rpcV2CborAddCapitalizedRoute = true`).
     *
     * With the setting enabled, the router also registers the legacy capitalized route.
     *
     * A camelCase operation like `getFoo` is then reachable through both `getFoo` and `GetFoo`.
     */
    @Test
    fun `opt-in registers both verbatim and legacy capitalized routes`() {
        val serviceShape = model.expectShape(ShapeId.from("test#Example"))
        serverIntegrationTest(
            model,
            params =
                IntegrationTestParams(
                    service = serviceShape.id.toString(),
                    additionalSettings =
                        ServerAdditionalSettings.builder()
                            .rpcV2CborAddCapitalizedRoute(true)
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

                    async fn my_api_op_handler(input: crate::input::MyApiOpInput) -> crate::output::MyApiOpOutput {
                        crate::output::MyApiOpOutput {
                            out: Some(format!("api: {}", input.param.unwrap_or_default())),
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

                tokioTest("optin_camel_case_verbatim_route_succeeds") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

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

                        assert!(response.status().is_success());
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                tokioTest("optin_capitalized_alias_route_succeeds") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"value": "test"}"##);

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

                        assert!(response.status().is_success());
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                tokioTest("optin_already_capitalized_operation_works") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"data": "hello"}"##);

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

                        assert!(response.status().is_success());
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }

                tokioTest("optin_acronym_capitalized_alias_route_succeeds") {
                    rustTemplate(
                        """
                        let config = crate::ExampleConfig::builder().build();
                        let service = crate::Example::builder(config)
                            .get_foo(get_foo_handler)
                            .upper_case_op(upper_case_op_handler)
                            .my_api_op(my_api_op_handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = create_cbor_input(r##"{"param": "test"}"##);

                        let request = #{Http}::Request::builder()
                            .uri("/service/Example/operation/MyAPIOp")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody:W})
                            .expect("Failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service, request)
                            .await
                            .expect("Failed to call service");

                        assert!(response.status().is_success());
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }
            }
        }
    }

    @Test
    fun `opt-in avoids request spec helper name collisions`() {
        val serviceShape = collidingOperationNamesModel.expectShape(ShapeId.from("test#ExampleCollision"))
        serverIntegrationTest(
            collidingOperationNamesModel,
            params =
                IntegrationTestParams(
                    service = serviceShape.id.toString(),
                    additionalSettings =
                        ServerAdditionalSettings.builder()
                            .rpcV2CborAddCapitalizedRoute(true)
                            .toObjectNode(),
                ),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    "SerdeJson" to CargoDependency.SerdeJson.toDevDependency().toType(),
                    "Ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
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
                            result: Some(format!("base: {}", input.value.unwrap_or_default())),
                        }
                    }

                    async fn get_foo_1_handler(input: crate::input::GetFoo1Input) -> crate::output::GetFoo1Output {
                        crate::output::GetFoo1Output {
                            result: Some(format!("suffix: {}", input.value.unwrap_or_default())),
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

                tokioTest("optin_collision_model_routes_capitalized_alias") {
                    rustTemplate(
                        """
                        let config = crate::ExampleCollisionConfig::builder().build();
                        let service = crate::ExampleCollision::builder(config)
                            .get_foo(get_foo_handler)
                            .get_foo_1(get_foo_1_handler)
                            .build()
                            .expect("could not build service");

                        let first_cbor_data = create_cbor_input(r##"{"value": "first"}"##);
                        let first_request = #{Http}::Request::builder()
                            .uri("/service/ExampleCollision/operation/GetFoo")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateFirstBody:W})
                            .expect("Failed to build request");

                        let first_response = #{Tower}::ServiceExt::oneshot(service.clone(), first_request)
                            .await
                            .expect("Failed to call service");
                        assert!(first_response.status().is_success());

                        #{ReadFirstBodyBytes:W}
                        let first_data: #{HashMap}<String, #{SerdeJson}::Value> =
                            #{Ciborium}::de::from_reader(first_body.as_ref()).expect("could not deserialize response");
                        let first_result = first_data.get("result")
                            .and_then(|r| r.as_str())
                            .expect("result field missing");
                        assert_eq!(first_result, "base: first");
                        """,
                        *codegenScope,
                        "CreateFirstBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "first_cbor_data"),
                        "ReadFirstBodyBytes" to ServerHttpTestHelpers.httpBodyToBytes(codegenContext.runtimeConfig, "first_body", "first_response"),
                    )
                }

                tokioTest("optin_collision_model_routes_suffix_operation") {
                    rustTemplate(
                        """
                        let config = crate::ExampleCollisionConfig::builder().build();
                        let service = crate::ExampleCollision::builder(config)
                            .get_foo(get_foo_handler)
                            .get_foo_1(get_foo_1_handler)
                            .build()
                            .expect("could not build service");

                        let second_cbor_data = create_cbor_input(r##"{"value": "second"}"##);
                        let second_request = #{Http}::Request::builder()
                            .uri("/service/ExampleCollision/operation/getFoo_1")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateSecondBody:W})
                            .expect("Failed to build request");

                        let second_response = #{Tower}::ServiceExt::oneshot(service, second_request)
                            .await
                            .expect("Failed to call service");
                        assert!(second_response.status().is_success());

                        #{ReadSecondBodyBytes:W}
                        let second_data: #{HashMap}<String, #{SerdeJson}::Value> =
                            #{Ciborium}::de::from_reader(second_body.as_ref()).expect("could not deserialize response");
                        let second_result = second_data.get("result")
                            .and_then(|r| r.as_str())
                            .expect("result field missing");
                        assert_eq!(second_result, "suffix: second");
                        """,
                        *codegenScope,
                        "CreateSecondBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "second_cbor_data"),
                        "ReadSecondBodyBytes" to ServerHttpTestHelpers.httpBodyToBytes(codegenContext.runtimeConfig, "second_body", "second_response"),
                    )
                }
            }
        }
    }
}
