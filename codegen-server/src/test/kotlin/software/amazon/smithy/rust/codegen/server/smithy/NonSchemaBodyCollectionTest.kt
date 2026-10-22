/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

internal class NonSchemaBodyCollectionTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace test.bodycollection
        use aws.protocols#restJson1
        @restJson1
        service CollectionService { version: "1", operations: [Echo, Upload, Stream] }
        @http(method: "POST", uri: "/echo")
        operation Echo {
            input := { @required message: String }
            output := { @required message: String }
        }
        @http(method: "POST", uri: "/upload")
        operation Upload {
            input := { @required @httpPayload data: Blob }
            output := { @required size: Long }
        }
        @streaming
        blob StreamedBlob
        @http(method: "POST", uri: "/stream")
        operation Stream {
            input := { @required @httpPayload data: StreamedBlob }
            output := { @required size: Long }
        }
        """.asSmithyModel()

    @Test
    fun `non schema collection preserves bounded unlimited and streaming responses`() {
        val observations = Files.createTempDirectory("smithy-body-collection-")
        println("Body collection observations: $observations")
        for (maxBytes in listOf(16, 0)) {
            for (version in HttpTestVersion.values()) {
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        overrideTestDir = Files.createTempDirectory("smithy-body-collection-crate-").toFile(),
                        additionalSettings =
                            ObjectNode.parse(
                                """{"codegen":{"schemaSerde":false,"requestBodyMaxBytes":$maxBytes}}""",
                            ).expectObjectNode(),
                        cargoCommand = "cargo test --quiet --lib --tests",
                    ),
                    testCoverage = HttpTestType.Only(version),
                ) { context, crate ->
                    check(!context.usesSchemaHttpSerde)
                    val snapshot = observations.resolve("$maxBytes-${context.runtimeConfig.httpVersion}.txt")
                    // Recorded from generated schemaSerde=false servers before moving collection
                    // into the HTTP 1.x internal module; both HTTP versions produced these responses.
                    val rejection =
                        """400 [("content-type", "application/json"), ("x-amzn-errortype", "SerializationException")] {}"""
                    val expected =
                        """
                        structured_under: 200 [("content-length", "16"), ("content-type", "application/json")] {"message":"ok"}
                        structured_over: ${if (maxBytes > 0) rejection else """200 [("content-length", "35"), ("content-type", "application/json")] {"message":"a much longer message"}"""}
                        structured_read_error: $rejection
                        payload_under: 200 [("content-length", "10"), ("content-type", "application/json")] {"size":2}
                        payload_over: ${if (maxBytes > 0) rejection else """200 [("content-length", "11"), ("content-type", "application/json")] {"size":34}"""}
                        payload_read_error: $rejection
                        streaming_over: 200 [("content-length", "11"), ("content-type", "application/json")] {"size":34}
                        """.trimIndent() + "\n"
                    val requestBody =
                        writable {
                            if (context.runtimeConfig.httpVersion == HttpVersion.Http1x) {
                                rustTemplate(
                                    """
                                    #{Server}::body::boxed_sync(#{BodyUtil}::StreamBody::new(
                                        #{Futures}::stream::iter(chunks.into_iter().map(|chunk| chunk.map(#{HttpBody}::Frame::data)))
                                    ))
                                    """,
                                    "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                    "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                    "HttpBody" to RuntimeType.httpBody(context.runtimeConfig),
                                    "Futures" to ServerCargoDependency.FuturesUtil.toType(),
                                )
                            } else {
                                rustTemplate(
                                    "#{Hyper}::Body::wrap_stream(#{Futures}::stream::iter(chunks))",
                                    "Hyper" to RuntimeType.hyper(context.runtimeConfig),
                                    "Futures" to ServerCargoDependency.FuturesUtil.toType(),
                                )
                            }
                        }
                    crate.testModule {
                        tokioTest("body_collection_contract") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                let service = crate::CollectionService::builder(crate::CollectionServiceConfig::builder().build())
                                    .echo(|input: crate::input::EchoInput| async move {
                                        #{Ok}::<_, crate::error::EchoError>(crate::output::EchoOutput { message: input.message })
                                    })
                                    .upload(|input: crate::input::UploadInput| async move {
                                        #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput { size: input.data.as_ref().len() as i64 })
                                    })
                                    .stream(|input: crate::input::StreamInput| async move {
                                        #{Ok}::<_, crate::error::StreamError>(crate::output::StreamOutput {
                                            size: input.data.collect().await.unwrap().into_bytes().len() as i64,
                                        })
                                    })
                                    .build().unwrap();
                                let mut records = #{String}::new();
                                for (name, uri, content_type, data, failed_read) in [
                                    ("structured_under", "/echo", "application/json", br##"{"message":"ok"}"##.as_slice(), false),
                                    ("structured_over", "/echo", "application/json", br##"{"message":"a much longer message"}"##.as_slice(), false),
                                    ("structured_read_error", "/echo", "application/json", b"".as_slice(), true),
                                    ("payload_under", "/upload", "application/octet-stream", b"ok".as_slice(), false),
                                    ("payload_over", "/upload", "application/octet-stream", b"0123456789012345678901234567890123".as_slice(), false),
                                    ("payload_read_error", "/upload", "application/octet-stream", b"".as_slice(), true),
                                    ("streaming_over", "/stream", "application/octet-stream", b"0123456789012345678901234567890123".as_slice(), false),
                                ] {
                                    let chunks: #{Vec}<#{Result}<#{Bytes}, ::std::io::Error>> =
                                        if failed_read {
                                            vec![#{Err}(::std::io::Error::other("transport failure"))]
                                        } else {
                                            data.chunks(8).map(|chunk| #{Ok}(#{Bytes}::copy_from_slice(chunk))).collect()
                                        };
                                    let request = #{Http}::Request::builder()
                                        .method("POST").uri(uri).header("content-type", content_type)
                                        .body(#{RequestBody}).unwrap();
                                    let response = service.clone().oneshot(request).await.unwrap();
                                    let status = response.status().as_u16();
                                    let mut headers: #{Vec}<_> = response.headers().iter()
                                        .map(|(name, value)| (name.as_str().to_owned(), value.to_str().unwrap().to_owned())).collect();
                                    headers.sort();
                                    #{ReadResponse}
                                    records.push_str(&format!("{name}: {status} {headers:?} {}\n", ::std::str::from_utf8(&bytes).unwrap()));
                                }
                                assert_eq!(records, ${expected.dq()});
                                ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                                """,
                                *RuntimeType.preludeScope,
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "Bytes" to RuntimeType.Bytes,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "RequestBody" to requestBody,
                                "ReadResponse" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "bytes", "response"),
                            )
                        }
                    }
                }
            }
        }
    }
}
