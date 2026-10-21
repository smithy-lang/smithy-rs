/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

internal class RestStreamingBlobInputCompatibilityTest {
    @Test
    fun `REST JSON exposes validated headers before body bytes arrive in legacy and schema serde`() {
        verifyProtocol("restJson1")
    }

    @Test
    fun `REST XML exposes validated headers before body bytes arrive in legacy and schema serde`() {
        verifyProtocol("restXml")
    }

    private fun verifyProtocol(protocol: String) {
        val captures = Files.createTempDirectory(Files.createDirectories(Path.of("../tools/schema-serde-compat/.runs")), "streaming-blob-input-").toAbsolutePath()
        for ((schema, publicTypes) in listOf(false to true, true to true, true to false)) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.upload
                @aws.protocols#$protocol
                service Uploads { version: "1", operations: [Upload, OptionalOnly] }
                @http(method: "POST", uri: "/upload")
                operation Upload {
                    input := {
                        @default(null) @httpPayload fahad: StreamingBlob
                        @required @httpHeader("x-title") title: Title
                    }
                    output := {}
                    errors: [smithy.framework#ValidationException]
                }
                @http(method: "POST", uri: "/optional")
                operation OptionalOnly { input := { @default(null) @httpPayload data: StreamingBlob }, output := {} }
                @length(min: 3) string Title
                @streaming blob StreamingBlob
            """.asSmithyModel()
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schema).publicConstrainedTypes(publicTypes).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schema)
                val scope =
                    arrayOf(
                        "Tokio" to CargoDependency.Tokio.toType(),
                        "Bytes" to CargoDependency.Bytes.toType(),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        *RuntimeType.preludeScope,
                    )
                crate.testModule {
                    tokioTest("headers_validate_without_polling_blob") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            use #{BodyUtil}::BodyExt;
                            for (case, title, expected) in [("valid", #{Some}("hello"), 200), ("short", #{Some}("a"), 400), ("missing", #{None}, 400)] {
                                let service = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                    .upload(|input: crate::input::UploadInput| async move {
                                        assert_eq!(input.title.as_str(), "hello");
                                        #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {})
                                    }).build_unchecked();
                                let mut request = #{Http}::Request::builder().method("POST").uri("/upload")
                                    .header("content-type", "application/octet-stream");
                                if let #{Some}(title) = title { request = request.header("x-title", title); }
                                let frames = #{Futures}::stream::poll_fn(|_| -> ::std::task::Poll<#{Option}<#{Result}<#{HttpBody}::Frame<#{Bytes}::Bytes>, #{Box}<dyn ::std::error::Error + Send + Sync>>>> {
                                    panic!("blob polled before header validation or handler");
                                });
                                let response = service.oneshot(request.body(#{BodyUtil}::StreamBody::new(frames)).unwrap()).await.unwrap();
                                assert_eq!(response.status(), expected);
                                let (parts, body) = response.into_parts();
                                let body = body.collect().await.unwrap().to_bytes();
                                ::std::fs::write(format!("$captures/$schema-$publicTypes-{case}.wire"), format!("status={}\nheaders={:?}\nbody={:?}\n", parts.status, parts.headers, body)).unwrap();
                                ::std::fs::write(format!("$captures/$schema-$publicTypes-{case}.body"), &body).unwrap();
                            }
                            """,
                            *scope,
                            "Futures" to CargoDependency.FuturesUtil.toType(),
                            "HttpBody" to CargoDependency.HttpBody1x.toType(),
                        )
                    }
                    tokioTest("live_large_blob") {
                        rustTemplate(
                            """
                            use #{Tokio}::io::{AsyncReadExt, AsyncWriteExt};
                            {
                                let entered = ::std::sync::Arc::new(#{Tokio}::sync::Notify::new());
                                let handler_entered = entered.clone();
                                let completed = ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false));
                                let handler_completed = completed.clone();
                                let service = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                    .upload(move |input: crate::input::UploadInput| {
                                        let entered = handler_entered.clone();
                                        let completed = handler_completed.clone();
                                        async move {
                                            assert_eq!(input.title.as_str(), "hello");
                                            entered.notify_one();
                                            let mut stream = input.fahad;
                                            let mut count = 0;
                                            while let #{Some}(chunk) = stream.next().await {
                                                let chunk = chunk.unwrap();
                                                assert!(chunk.iter().all(|byte| *byte == b'x'));
                                                count += chunk.len();
                                            }
                                            assert_eq!(count, 1024 * 1024);
                                            completed.store(true, ::std::sync::atomic::Ordering::SeqCst);
                                            #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {})
                                        }
                                    }).build_unchecked();
                                let listener = #{Tokio}::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                                let address = listener.local_addr().unwrap();
                                let server = #{Tokio}::spawn(async move { crate::serve(listener, service.into_make_service()).await.unwrap(); });
                                let mut socket = #{Tokio}::net::TcpStream::connect(address).await.unwrap();
                                let headers = "POST /upload HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nTransfer-Encoding: chunked\r\nContent-Type: application/octet-stream\r\nx-title: hello\r\n\r\n";
                                socket.write_all(headers.as_bytes()).await.unwrap();
                                {
                                    // The client has not sent blob bytes or EOF. Collection here would deadlock.
                                    let mut early = [0u8; 4096];
                                    #{Tokio}::time::timeout(::std::time::Duration::from_secs(5), async {
                                        #{Tokio}::select! {
                                            _ = entered.notified() => {},
                                            read = socket.read(&mut early) => panic!("early response: {:?}", &early[..read.unwrap()]),
                                        }
                                    }).await.unwrap_or_else(|_| panic!("handler not entered"));
                                }
                                let payload = vec![b'x'; 16 * 1024];
                                for _ in 0..64 {
                                    socket.write_all(b"4000\r\n").await.unwrap();
                                    socket.write_all(&payload).await.unwrap();
                                    socket.write_all(b"\r\n").await.unwrap();
                                }
                                socket.write_all(b"0\r\n\r\n").await.unwrap();
                                let mut response = #{Vec}::new();
                                #{Tokio}::time::timeout(::std::time::Duration::from_secs(5), socket.read_to_end(&mut response)).await.unwrap().unwrap();
                                ::std::fs::write(format!("$captures/$schema-$publicTypes-live.http"), &response).unwrap();
                                assert!(response.starts_with(b"HTTP/1.1 200"), "{:?}", response);
                                assert!(completed.load(::std::sync::atomic::Ordering::SeqCst));
                                server.abort();
                            }
                            """,
                            *scope,
                        )
                    }
                }
            }
        }
        for (case in listOf("valid", "short", "missing")) {
            val legacy = Files.readAllLines(captures.resolve("false-true-$case.wire"))
            val legacyBody = Files.readAllBytes(captures.resolve("false-true-$case.body"))
            for (publicTypes in listOf(true, false)) {
                val actual = Files.readAllLines(captures.resolve("true-$publicTypes-$case.wire"))
                val actualBody = Files.readAllBytes(captures.resolve("true-$publicTypes-$case.body"))
                check(actual.take(2) == legacy.take(2)) { "$protocol $case status or headers differ" }
                // Keep the raw generated serde and schema serde captures. JSON object member order is insignificant.
                if (protocol == "restJson1") {
                    check(ObjectNode.parse(actualBody.decodeToString()) == ObjectNode.parse(legacyBody.decodeToString())) {
                        "$protocol $case JSON response differs"
                    }
                } else {
                    check(actualBody.contentEquals(legacyBody)) { "$protocol $case XML response differs" }
                }
            }
        }
    }
}
