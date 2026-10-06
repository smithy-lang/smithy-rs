/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.assertions.throwables.shouldThrowAny
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

class MultiProtocolSchemaServiceTest {
    private val schemaSerde =
        IntegrationTestParams(additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode())

    private val model =
        """
        namespace test

        use aws.protocols#awsJson1_0
        use aws.protocols#awsJson1_1
        use aws.protocols#restJson1
        use aws.protocols#restXml
        use smithy.protocols#rpcv2Cbor
        use smithy.test#httpRequestTests
        use smithy.test#httpResponseTests

        @rpcv2Cbor
        @awsJson1_0
        @awsJson1_1
        @restJson1
        @restXml
        service Example { operations: [Greet, Upload] }

        @http(method: "POST", uri: "/greet", code: 201)
        @httpRequestTests([
            {
                id: "MultiProtocolGreetAwsJson10Request",
                protocol: awsJson1_0,
                method: "POST",
                uri: "/",
                headers: { "Content-Type": "application/x-amz-json-1.0", "X-Amz-Target": "Example.Greet" },
                body: "{\"name\":\"n\"}",
                bodyMediaType: "application/json",
                params: { name: "n" },
            },
            {
                id: "MultiProtocolGreetRestJsonRequest",
                protocol: restJson1,
                method: "POST",
                uri: "/greet",
                headers: { "Content-Type": "application/json" },
                body: "{\"name\":\"n\"}",
                bodyMediaType: "application/json",
                params: { name: "n" },
            },
        ])
        @httpResponseTests([
            {
                id: "MultiProtocolGreetAwsJson10Response",
                protocol: awsJson1_0,
                code: 200,
                body: "{\"message\":\"hi\"}",
                bodyMediaType: "application/json",
                params: { message: "hi" },
            },
            {
                id: "MultiProtocolGreetRestJsonResponse",
                protocol: restJson1,
                code: 201,
                body: "{\"message\":\"hi\"}",
                bodyMediaType: "application/json",
                params: { message: "hi" },
            },
        ])
        operation Greet {
            input := { name: String }
            output := { message: String }
        }

        @idempotent
        @http(method: "PUT", uri: "/upload")
        operation Upload {
            input := { @httpPayload @required data: StreamingBlob }
            output := {}
            errors: [smithy.framework#ValidationException]
        }

        @streaming
        blob StreamingBlob
        """.asSmithyModel(smithyVersion = "2")

    @Test
    fun `one service serves every declared protocol`() {
        serverIntegrationTest(
            model,
            schemaSerde,
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            val scope =
                arrayOf(
                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                    "Http" to RuntimeType.http(context.runtimeConfig),
                    "Tower" to ServerCargoDependency.Tower.toType(),
                    "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                    "Bytes" to RuntimeType.Bytes,
                    *RuntimeType.preludeScope,
                )
            crate.testModule {
                rustTemplate(
                    """
                    use #{Tower}::ServiceExt;
                    use #{BodyUtil}::BodyExt;

                    fn service() -> crate::Example {
                        crate::Example::builder(crate::ExampleConfig::builder().build())
                            .greet(|input: crate::input::GreetInput| async move {
                                crate::output::GreetOutput { message: input.name.map(|name| format!("hello {name}")) }
                            })
                            .upload(|input: crate::input::UploadInput| async move {
                                let data = input.data.collect().await.unwrap().into_bytes();
                                assert_eq!(&data[..], b"payload");
                                #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {})
                            })
                            .build()
                            .unwrap()
                    }

                    async fn send(request: #{Http}::request::Builder, body: &str) -> (u16, #{Http}::HeaderMap, #{String}) {
                        let body = #{Bytes}::copy_from_slice(body.as_bytes());
                        let request = request.body(#{Server}::body::Body::from_bytes(body)).unwrap();
                        let response = service().oneshot(request).await.unwrap();
                        let status = response.status().as_u16();
                        let headers = response.headers().clone();
                        let body = response.into_body().collect().await.unwrap().to_bytes();
                        (status, headers, #{String}::from_utf8_lossy(&body).into_owned())
                    }

                    fn post(uri: &str) -> #{Http}::request::Builder {
                        #{Http}::Request::builder().method("POST").uri(uri)
                    }
                    """,
                    *scope,
                )
                tokioTest("each_protocol_claims_its_own_requests") {
                    rustTemplate(
                        """
                        // REST protocols honor `@http(code: 201)`; RPC protocols answer 200 whatever the model says.
                        let (status, headers, body) =
                            send(post("/greet").header("content-type", "application/json"), r##"{"name":"json"}"##).await;
                        assert_eq!((status, headers["content-type"].to_str().unwrap()), (201, "application/json"));
                        assert!(body.contains("hello json"), "{body}");

                        let (status, headers, body) = send(
                            post("/greet").header("content-type", "application/xml"),
                            "<GreetInput><name>xml</name></GreetInput>",
                        )
                        .await;
                        assert_eq!((status, headers["content-type"].to_str().unwrap()), (201, "application/xml"));
                        assert!(body.contains("hello xml"), "{body}");

                        for (content_type, name) in [("application/x-amz-json-1.0", "one-zero"), ("application/x-amz-json-1.1", "one-one")] {
                            let request = post("/").header("content-type", content_type).header("x-amz-target", "Example.Greet");
                            let (status, headers, text) = send(request, &format!(r##"{{"name":"{name}"}}"##)).await;
                            assert_eq!((status, headers["content-type"].to_str().unwrap()), (200, content_type));
                            assert!(text.contains(&format!("hello {name}")), "{text}");
                        }

                        let (status, headers, _) = send(
                            post("/service/Example/operation/Greet")
                                .header("smithy-protocol", "rpc-v2-cbor")
                                .header("content-type", "application/cbor"),
                            "",
                        )
                        .await;
                        assert_eq!((status, headers["content-type"].to_str().unwrap()), (200, "application/cbor"));
                        """,
                        *scope,
                    )
                }
                tokioTest("an_operation_a_protocol_cannot_serve_is_rejected_by_that_protocol_only") {
                    rustTemplate(
                        """
                        // rpcv2Cbor does not stream blobs: it claims the request and rejects it.
                        let (status, _, _) = send(
                            post("/service/Example/operation/Upload").header("smithy-protocol", "rpc-v2-cbor"),
                            "payload",
                        )
                        .await;
                        assert_eq!(status, 404);
                        let (status, _, _) = send(#{Http}::Request::builder().method("PUT").uri("/upload")
                            .header("content-type", "application/octet-stream"), "payload").await;
                        assert_eq!(status, 200);
                        """,
                        *scope,
                    )
                }
                tokioTest("a_request_no_protocol_claims_is_answered_like_coral") {
                    rustTemplate(
                        """
                        for request in [
                            post("/").header("x-amz-target", "Example.Unknown"),
                            post("/greet").header("content-type", "text/plain"),
                            #{Http}::Request::builder().method("GET").uri("/nowhere"),
                        ] {
                            let (status, headers, body) = send(request, "{}").await;
                            assert_eq!(status, 404);
                            assert!(headers.is_empty());
                            assert_eq!(body, "<UnknownOperationException/>\n");
                        }
                        // A recognizable AWS JSON media type offers a deferred rejection,
                        // even though an unknown operation cannot satisfy strict claiming.
                        let (status, headers, body) = send(
                            post("/").header("content-type", "application/x-amz-json-1.0")
                                .header("x-amz-target", "Example.Unknown"), "{}",
                        ).await;
                        assert_eq!(status, 404);
                        assert_eq!(headers["content-type"], "application/x-amz-json-1.0");
                        assert!(body.is_empty());
                        """,
                        *scope,
                    )
                }
            }
        }
    }

    @Test
    fun `a declared protocol code generation does not support fails the build`() {
        val query =
            """
            namespace test
            use aws.protocols#awsQuery
            use aws.protocols#awsJson1_0
            @awsJson1_0
            @awsQuery
            @xmlNamespace(uri: "https://example.com")
            service Example { version: "2020-01-01", operations: [Ping] }
            operation Ping {}
            """.asSmithyModel(smithyVersion = "2")
        val error =
            shouldThrowAny {
                serverIntegrationTest(query, schemaSerde, testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X))
            }
        error.message shouldContain "aws.protocols#awsQuery"
    }
}
