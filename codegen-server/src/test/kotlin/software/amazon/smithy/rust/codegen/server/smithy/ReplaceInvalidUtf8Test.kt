package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ReplaceInvalidUtf8Test {
    val model =
        """
        namespace test
        use aws.protocols#restJson1

        @restJson1
        service SampleService {
            operations: [SampleOperation]
        }

        @http(uri: "/operation", method: "PUT")
        operation SampleOperation {
            input := {
                x : String
            }
        }
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `invalid utf8 should be replaced if the codegen flag is set`() {
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
                            .header("content-type", "application/json")
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

    @Test
    fun `invalid utf8 should be rejected if the codegen flag is not set`() {
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
                            .header("content-type", "application/json")
                            .body(hyper::Body::from(body))
                            .expect("failed to build request");
                        let result = crate::protocol_serde::shape_sample_operation::de_sample_operation_http_request(request).await;
                        assert!(
                            result.is_err(),
                            "invalid utf8 characters should not be allowed by default {result:?}"
                        );
                    }
                    """,
                )
            }
        }
    }
}
