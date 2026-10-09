/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators.http

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * A client may send a request header value that is not valid UTF-8 — HTTP permits any octet except a
 * control character in a header value.
 *
 * `Headers` accepts such a value rather than failing the whole request at the HTTP layer, and the
 * encoding requirement is applied where the value is bound to a modeled member. On the server that
 * must surface as a rejected request, not as the member silently deserializing to `None`: a server
 * that branches on a header being absent would otherwise take the absent branch on input the client
 * actually sent.
 *
 * The client-side `NonUtf8HeaderHandling` setting deliberately does not reach here — the server has
 * no config bag, and relaxing this by default would turn a fail-closed into a fail-open.
 */
class NonUtf8RequestHeaderTest {
    private val model =
        """
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            version: "2023-01-01",
            operations: [SomeOperation],
        }

        @http(uri: "/operation", method: "POST")
        operation SomeOperation {
            input: SomeInput,
            output: SomeOutput,
        }

        structure SomeInput {
            @httpHeader("x-header")
            header: String,
        }

        structure SomeOutput {
            message: String,
        }
        """.asSmithyModel()

    @Test
    fun `a non-UTF-8 request header is rejected rather than read as absent`() {
        serverIntegrationTest(
            model,
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { codegenContext, rustCrate ->
            val codegenScope =
                arrayOf(
                    *ServerHttpTestHelpers.getHttpRuntimeTypeScope(codegenContext),
                    "Boxed" to
                        ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig)
                            .toType().resolve("body::boxed"),
                    "Bytes" to RuntimeType.Bytes,
                    "Full" to CargoDependency.HttpBodyUtil01x.toType().resolve("Full"),
                    "Tower" to RuntimeType.Tower,
                    *RuntimeType.preludeScope,
                )

            rustCrate.testModule {
                rustTemplate(
                    """
                    async fn some_operation_handler(
                        _input: crate::input::SomeOperationInput,
                    ) -> crate::output::SomeOperationOutput {
                        crate::output::SomeOperationOutput { message: #{None} }
                    }

                    fn service() -> crate::TestService {
                        let config = crate::TestServiceConfig::builder().build();
                        crate::TestService::builder(config)
                            .some_operation(some_operation_handler)
                            .build()
                            .expect("could not build service")
                    }
                    """,
                    *codegenScope,
                )

                tokioTest("non_utf8_request_header_is_rejected") {
                    rustTemplate(
                        """
                        // A lone 0xE9 is a valid HTTP header octet (obs-text per RFC 7230) but is not
                        // valid UTF-8.
                        let request = #{Http}::Request::builder()
                            .uri("/operation")
                            .method("POST")
                            .header("content-type", "application/json")
                            .header(
                                "x-header",
                                #{Http}::HeaderValue::from_bytes(b"value-\xe9").unwrap(),
                            )
                            .body(#{Boxed}(#{Full}::new(#{Bytes}::from("{}"))))
                            .expect("failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service(), request)
                            .await
                            .expect("failed to call service");

                        assert_eq!(#{Http}::StatusCode::BAD_REQUEST, response.status());
                        """,
                        *codegenScope,
                    )
                }

                tokioTest("a_valid_request_header_is_still_accepted") {
                    rustTemplate(
                        """
                        let request = #{Http}::Request::builder()
                            .uri("/operation")
                            .method("POST")
                            .header("content-type", "application/json")
                            .header("x-header", "readable")
                            .body(#{Boxed}(#{Full}::new(#{Bytes}::from("{}"))))
                            .expect("failed to build request");

                        let response = #{Tower}::ServiceExt::oneshot(service(), request)
                            .await
                            .expect("failed to call service");

                        assert_eq!(#{Http}::StatusCode::OK, response.status());
                        """,
                        *codegenScope,
                    )
                }
            }
        }
    }
}
