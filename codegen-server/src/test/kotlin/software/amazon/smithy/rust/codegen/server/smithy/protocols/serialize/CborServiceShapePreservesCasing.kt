/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.serialize

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

class CborServiceShapePreservesCasing {
    val model =
        """
        namespace test

        use smithy.rust#serde
        use smithy.protocols#rpcv2Cbor
        use smithy.framework#ValidationException

        @rpcv2Cbor
        service SampleServiceWITHDifferentCASE {
            operations: [SampleOP],
        }
        operation SampleOP {
            input:= { x: String }
            output:= { y: String }
        }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `service shape ID is preserved`() {
        val serviceShape = model.expectShape(ShapeId.from("test#SampleServiceWITHDifferentCASE"))
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
                    async fn handler(input: crate::input::SampleOpInput) -> crate::output::SampleOpOutput {
                        assert_eq!(
                            input.x.expect("missing value for x"),
                            "test",
                            "input does not contain the correct data"
                        );
                        crate::output::SampleOpOutput {
                            y: Some("test response".to_owned()),
                        }
                    }

                    fn get_input() -> Vec<u8> {
                        let json = r##"{"x": "test"}"##;
                        let value: #{SerdeJson}::Value = #{SerdeJson}::from_str(json).expect("cannot parse JSON");
                        let mut cbor_data = #{Vec}::new();
                        #{Ciborium}::ser::into_writer(&value, &mut cbor_data)
                            .expect("cannot write JSON to CBOR");
                        cbor_data
                    }
                    """,
                    *codegenScope,
                )

                tokioTest("success_response") {
                    rustTemplate(
                        """
                        let config = crate::SampleServiceWithDifferentCaseConfig::builder().build();
                        let service = crate::SampleServiceWithDifferentCase::builder(config)
                            .sample_op(handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = get_input();
                        // Create a test request
                        let request = #{Http}::Request::builder()
                            .uri("/service/SampleServiceWITHDifferentCASE/operation/SampleOP")
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
                        let data: #{HashMap}<String, serde_json::Value> =
                            #{Ciborium}::de::from_reader(body.as_ref()).expect("could not convert into BTreeMap");

                        let value = data.get("y")
                            .and_then(|y| y.as_str())
                            .expect("y does not exist");
                        assert_eq!(value, "test response", "response doesn't contain expected value");
                        """,
                        *codegenScope,
                        "CreateBody" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                        "ReadBodyBytes" to ServerHttpTestHelpers.httpBodyToBytes(codegenContext.runtimeConfig, "body", "response"),
                    )
                }

                tokioTest("incorrect_case_fails") {
                    rustTemplate(
                        """
                        let config = crate::SampleServiceWithDifferentCaseConfig::builder().build();
                        let service = crate::SampleServiceWithDifferentCase::builder(config)
                            .sample_op(handler)
                            .build()
                            .expect("could not build service");

                        let cbor_data = get_input();
                        // Test with incorrect case in service name
                        let request = #{Http}::Request::builder()
                            .uri("/service/SampleServiceWithDifferentCase/operation/SampleOP")
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody1:W})
                            .expect("failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service.clone(), request)
                            .await
                            .expect("failed to call service");

                        // Should return 404 Not Found
                        assert_eq!(response.status(), #{Http}::StatusCode::NOT_FOUND);

                        // Test with incorrect case in operation name
                        let request = #{Http}::Request::builder()
                            .uri("/service/SampleServiceWITHDifferentCASE/operation/sampleop")  // lowercase operation
                            .method("POST")
                            .header("content-type", "application/cbor")
                            .header("Smithy-Protocol", "rpc-v2-cbor")
                            .body(#{CreateBody2:W})
                            .expect("failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service, request)
                            .await
                            .expect("failed to call service");

                        // Should return 404 Not Found
                        assert_eq!(response.status(), #{Http}::StatusCode::NOT_FOUND);
                        """,
                        *codegenScope,
                        "CreateBody1" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data.clone()"),
                        "CreateBody2" to ServerHttpTestHelpers.createBodyFromBytes(codegenContext, "cbor_data"),
                    )
                }
            }
        }
    }
}
