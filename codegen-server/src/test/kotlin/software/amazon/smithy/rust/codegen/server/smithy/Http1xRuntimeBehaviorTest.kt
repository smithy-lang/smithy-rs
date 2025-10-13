/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/**
 * Tests for http-1x flag runtime behavior.
 * Phase 3: Runtime - verifies that generated code works correctly at runtime with both HTTP/0 and HTTP/1.
 */
internal class Http1xRuntimeBehaviorTest {
    private fun buildAdditionalSettings(http1x: Boolean): ObjectNode =
        Node.objectNodeBuilder()
            .withMember(
                "codegen",
                Node.objectNodeBuilder()
                    .withMember("http-1x", http1x)
                    .build(),
            ).build()

    private val testModel =
        """
        namespace test

        use aws.protocols#restJson1

        @restJson1
        service TestService {
            version: "2024-03-18"
            operations: [Echo, GetStatus]
        }

        @http(uri: "/echo", method: "POST")
        operation Echo {
            input: EchoInput
            output: EchoOutput
        }

        @http(uri: "/status", method: "GET")
        operation GetStatus {
            output: GetStatusOutput
        }

        structure EchoInput {
            @required
            message: String

            @httpHeader("X-Request-Id")
            requestId: String
        }

        structure EchoOutput {
            @required
            message: String

            @httpHeader("X-Response-Id")
            responseId: String
        }

        structure GetStatusOutput {
            @required
            status: String
        }
        """.asSmithyModel()

    @Test
    fun `HTTP types are compatible with http-1x disabled`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.unitTest("http_types_compatibility_http0") {
                rustTemplate(
                    """
                    use #{http};

                    // Verify http 0.2 types are available
                    let _uri: #{http}::Uri = "/test".parse().unwrap();
                    let _method = #{http}::Method::GET;
                    let _status = #{http}::StatusCode::OK;
                    let _version = #{http}::Version::HTTP_11;
                    """,
                    "http" to codegenContext.httpDependencies().httpModule(),
                )
            }
        }
    }

    @Test
    fun `HTTP types are compatible with http-1x enabled`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.unitTest("http_types_compatibility_http1") {
                rustTemplate(
                    """
                    use #{http};

                    // Verify http 1.x types are available
                    let _uri: #{http}::Uri = "/test".parse().unwrap();
                    let _method = #{http}::Method::GET;
                    let _status = #{http}::StatusCode::OK;
                    let _version = #{http}::Version::HTTP_11;
                    """,
                    "http" to codegenContext.httpDependencies().httpModule(),
                )
            }
        }
    }

    @Test
    fun `Request types work correctly with http-1x disabled`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.unitTest("request_types_http0") {
                rustTemplate(
                    """
                    use #{http};
                    use crate::{input, output};

                    // Create a request using http 0.2 types
                    let request_builder = #{http}::Request::builder()
                        .method(#{http}::Method::POST)
                        .uri("/echo")
                        .header("content-type", "application/json")
                        .header("X-Request-Id", "test-123");

                    let body = r#"{"message": "hello"}"#;
                    let _request = request_builder
                        .body(body.as_bytes().to_vec())
                        .unwrap();

                    // Verify we can construct input/output types
                    let echo_input = input::EchoInput::builder()
                        .message("test".to_string())
                        .request_id("req-123".to_string())
                        .build()
                        .unwrap();

                    assert_eq!(echo_input.message, "test");
                    assert_eq!(echo_input.request_id, Some("req-123".to_string()));
                    """,
                    "http" to codegenContext.httpDependencies().httpModule(),
                )
            }
        }
    }

    @Test
    fun `Request types work correctly with http-1x enabled`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.unitTest("request_types_http1") {
                rustTemplate(
                    """
                    use #{http};
                    use crate::{input, output};

                    // Create a request using http 1.x types
                    let request_builder = #{http}::Request::builder()
                        .method(#{http}::Method::POST)
                        .uri("/echo")
                        .header("content-type", "application/json")
                        .header("X-Request-Id", "test-123");

                    let body = r#"{"message": "hello"}"#;
                    let _request = request_builder
                        .body(body.as_bytes().to_vec())
                        .unwrap();

                    // Verify we can construct input/output types
                    let echo_input = input::EchoInput::builder()
                        .message("test".to_string())
                        .request_id("req-123".to_string())
                        .build()
                        .unwrap();

                    assert_eq!(echo_input.message, "test");
                    assert_eq!(echo_input.request_id, Some("req-123".to_string()));
                    """,
                    "http" to codegenContext.httpDependencies().httpModule(),
                )
            }
        }
    }

    @Test
    fun `Response headers work correctly with http-1x disabled`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.unitTest("response_headers_http0") {
                rustTemplate(
                    """
                    use #{http};
                    use crate::output;

                    // Create output with headers
                    let echo_output = output::EchoOutput::builder()
                        .message("response message".to_string())
                        .response_id("resp-456".to_string())
                        .build()
                        .unwrap();

                    assert_eq!(echo_output.message, "response message");
                    assert_eq!(echo_output.response_id, Some("resp-456".to_string()));

                    // Verify HeaderMap is available
                    let mut headers = #{http}::HeaderMap::new();
                    headers.insert(
                        #{http}::header::CONTENT_TYPE,
                        #{http}::HeaderValue::from_static("application/json")
                    );
                    assert_eq!(headers.len(), 1);
                    """,
                    "http" to codegenContext.httpDependencies().httpModule(),
                )
            }
        }
    }

    @Test
    fun `Response headers work correctly with http-1x enabled`() {
        serverIntegrationTest(
            testModel,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        ) { codegenContext, rustCrate ->
            rustCrate.unitTest("response_headers_http1") {
                rustTemplate(
                    """
                    use #{http};
                    use crate::output;

                    // Create output with headers
                    let echo_output = output::EchoOutput::builder()
                        .message("response message".to_string())
                        .response_id("resp-456".to_string())
                        .build()
                        .unwrap();

                    assert_eq!(echo_output.message, "response message");
                    assert_eq!(echo_output.response_id, Some("resp-456".to_string()));

                    // Verify HeaderMap is available
                    let mut headers = #{http}::HeaderMap::new();
                    headers.insert(
                        #{http}::header::CONTENT_TYPE,
                        #{http}::HeaderValue::from_static("application/json")
                    );
                    assert_eq!(headers.len(), 1);
                    """,
                    "http" to codegenContext.httpDependencies().httpModule(),
                )
            }
        }
    }

    @Test
    fun `Builder patterns work identically for both HTTP versions`() {
        val testBothVersions = { http1x: Boolean ->
            serverIntegrationTest(
                testModel,
                IntegrationTestParams(
                    additionalSettings = buildAdditionalSettings(http1x),
                ),
            ) { _, rustCrate ->
                rustCrate.unitTest("builder_patterns_http${if (http1x) "1" else "0"}") {
                    rustTemplate(
                        """
                        use crate::{input, output};

                        // Test builder with all fields
                        let input_with_all = input::EchoInput::builder()
                            .message("test message".to_string())
                            .request_id("req-id".to_string())
                            .build()
                            .unwrap();

                        assert_eq!(input_with_all.message, "test message");
                        assert_eq!(input_with_all.request_id, Some("req-id".to_string()));

                        // Test builder with only required fields
                        let input_required_only = input::EchoInput::builder()
                            .message("required only".to_string())
                            .build()
                            .unwrap();

                        assert_eq!(input_required_only.message, "required only");
                        assert_eq!(input_required_only.request_id, None);

                        // Test output builder
                        let output_with_all = output::EchoOutput::builder()
                            .message("output message".to_string())
                            .response_id("resp-id".to_string())
                            .build()
                            .unwrap();

                        assert_eq!(output_with_all.message, "output message");
                        assert_eq!(output_with_all.response_id, Some("resp-id".to_string()));
                        """,
                    )
                }
            }
        }

        testBothVersions(false)
        testBothVersions(true)
    }
}
