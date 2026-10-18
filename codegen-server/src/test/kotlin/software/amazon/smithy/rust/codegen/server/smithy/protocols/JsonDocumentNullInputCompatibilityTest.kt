/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class JsonDocumentNullInputCompatibilityTest {
    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "awsJson1_0", "awsJson1_1"])
    fun `legacy handlers receive explicit document null`(protocol: String) = checkInputs(protocol, schemaSerde = false)

    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "awsJson1_0", "awsJson1_1"])
    fun `schema handler inputs match legacy document null`(protocol: String) = checkInputs(protocol, schemaSerde = true)

    private fun checkInputs(
        protocol: String,
        schemaSerde: Boolean,
    ) {
        val contentType =
            when (protocol) {
                "restJson1" -> "application/json"
                "awsJson1_0" -> "application/x-amz-json-1.0"
                else -> "application/x-amz-json-1.1"
            }
        val model =
            """
                ${'$'}version: "2"
                namespace test.documentnull
                use aws.protocols#$protocol
                @$protocol
                service Documents { version: "1", operations: [Inspect] }
                @http(method: "POST", uri: "/document")
                operation Inspect {
                    input := { value: Document }
                    output := {}
                }
                """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib -- --nocapture",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("handler_document_null_is_not_absence") {
                    rustTemplate(
                        """
                            use std::sync::{Arc, Mutex};
                            let inputs = Arc::new(Mutex::new(#{Vec}::new()));
                            let received = inputs.clone();
                            let config = crate::service::DocumentsConfig::builder().build();
                            let service = crate::service::Documents::builder(config)
                                .inspect(move |input: crate::input::InspectInput| {
                                    received.lock().unwrap().push(input.value);
                                    async { crate::output::InspectOutput::builder().build() }
                                })
                                .build_unchecked();
                            for (body, expected) in [
                                (b"{\"value\":null}".as_slice(), #{Some}(#{Document}::Null)),
                                (b"{}".as_slice(), #{None}),
                                (b"{\"value\":true}".as_slice(), #{Some}(#{Document}::Bool(true))),
                                (b"{\"value\":[null,false]}".as_slice(), #{Some}(#{Document}::Array(vec![#{Document}::Null, #{Document}::Bool(false)]))),
                            ] {
                                let request = #{Http}::Request::builder().method("POST")
                                    .uri("${if (protocol == "restJson1") "/document" else "/"}")
                                    .header("content-type", "$contentType")
                                    .header("x-amz-target", "Documents.Inspect")
                                    .body(#{Server}::body::to_boxed_sync(body)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let headers = response.headers().clone();
                                let response_body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                                let actual = inputs.lock().unwrap().pop().expect("handler ran");
                                println!("$protocol schemaSerde=$schemaSerde {body:?}: {status} {headers:?} {response_body:?}; input {actual:?}");
                                assert_eq!(status, 200);
                                assert_eq!(response_body.as_ref(), b"{}");
                                assert_eq!(actual, expected, "{body:?}");
                            }
                            """,
                        *preludeScope,
                        "Document" to RuntimeType.document(context.runtimeConfig),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                    )
                }
            }
        }
    }
}
