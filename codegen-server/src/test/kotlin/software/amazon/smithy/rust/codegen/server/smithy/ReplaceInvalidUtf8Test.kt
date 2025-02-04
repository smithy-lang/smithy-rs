/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ModelProtocol
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.forAllProtocols
import software.amazon.smithy.rust.codegen.core.testutil.forProtocols
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ReplaceInvalidUtf8Test {
    val model =
        """
        namespace test

        service SampleService {
            operations: [SampleOperation]
        }

        @http(uri: "/operation", method: "PUT")
        operation SampleOperation {
            input := {
                x : String
            }
        }
        """

    @Test
    fun `invalid utf8 should be replaced if the codegen flag is set`() {
        model.forProtocols(ModelProtocol.AwsJson10, ModelProtocol.AwsJson11, ModelProtocol.RestJson) { model, metadata ->
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        ServerAdditionalSettings
                            .builder()
                            .replaceInvalidUtf8(true)
                            .toObjectNode(),
                ),
            ) { _, rustCrate ->
                rustCrate.testModule {
                    rustTemplate(
                        """
                        ##[tokio::test]
                        async fn test_utf8_replaced() {
                            let body = r##"{ "x" : "\ud800" }"##;
                            let request = http::Request::builder()
                                .method("POST")
                                .uri("/operation")
                                .header("content-type", "${metadata.contentType}")
                                .body(hyper::Body::from(body))
                                .expect("failed to build request");
                            let result = crate::protocol_serde::shape_sample_operation::de_sample_operation_http_request(request).await;
                            assert!(
                                result.is_ok(),
                                "Invalid utf8 should have been replaced. {result:?}"
                            );
                            assert_eq!(
                                result.unwrap().x.unwrap(),
                                "�",
                                "payload should have been replaced with �."
                            );
                        }
                        """,
                    )
                }
            }
        }
    }

    @Test
    fun `invalid utf8 should be rejected if the codegen flag is not set`() {
        model.forAllProtocols(exclude = setOf(ModelProtocol.RestXml, ModelProtocol.Rpcv2Cbor)) { model, metadata ->
            serverIntegrationTest(
                model,
            ) { _, rustCrate ->
                rustCrate.testModule {
                    rustTemplate(
                        """
                        ##[tokio::test]
                        async fn test_invalid_utf8_raises_an_error() {
                            let body = r##"{ "x" : "\ud800" }"##;
                            let request = http::Request::builder()
                                .method("POST")
                                .uri("/operation")
                                .header("content-type", "${metadata.contentType}")
                                .body(hyper::Body::from(body))
                                .expect("failed to build request");
                            let result = crate::protocol_serde::shape_sample_operation::de_sample_operation_http_request(request).await;
                            assert!(
                                result.is_err(),
                                "invalid utf8 characters should not be allowed by default {result:?}"
                            );
                            let error_msg = result.err().unwrap().to_string();
                            assert!(error_msg.contains("failed to unescape JSON string"));
                        }
                        """,
                    )
                }
            }
        }
    }
}
