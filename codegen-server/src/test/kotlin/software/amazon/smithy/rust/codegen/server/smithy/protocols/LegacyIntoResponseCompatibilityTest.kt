/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class LegacyIntoResponseCompatibilityTest {
    @Test
    fun `legacy modeled errors retain protocol-specific IntoResponse without routing context`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.legacyresponse
            use aws.protocols#restJson1

            @restJson1
            service Detection { version: "1", operations: [ScanSbom] }

            @http(method: "POST", uri: "/scan")
            operation ScanSbom {
                input := {}
                output := {}
                errors: [ThrottlingException]
            }

            @error("client")
            @httpError(429)
            structure ThrottlingException {
                @required
                message: String
            }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":false}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet --lib",
            ),
        ) { context, crate ->
            check(!context.usesSchemaHttpSerde)
            crate.testModule {
                tokioTest("admission_error_into_response_without_routing_context") {
                    rustTemplate(
                        """
                        use crate::server::protocol::rest_json_1::RestJson1;
                        use crate::server::response::{IntoResponse, Response};

                        fn scan_throttle(message: &str) -> crate::error::ScanSbomError {
                            crate::error::ScanSbomError::ThrottlingException(crate::error::ThrottlingException {
                                message: message.into(),
                            })
                        }

                        let response: Response = IntoResponse::<RestJson1>::into_response(scan_throttle(
                            "too many concurrent scans in flight; retry with backoff",
                        ));
                        assert_eq!(response.status().as_u16(), 429);
                        assert_eq!(response.headers()["content-type"], "application/json");
                        assert_eq!(response.headers()["x-amzn-errortype"], "ThrottlingException");
                        #{Collect}
                        assert_eq!(
                            body.as_ref(),
                            b"{\"message\":\"too many concurrent scans in flight; retry with backoff\"}",
                        );
                        """,
                        "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                    )
                }
            }
        }
    }
}
