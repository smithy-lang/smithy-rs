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
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

internal class StreamingBlobInputTest {
    @Test
    fun `live input preserves ordinary fields and defers validation with public and private builders`() {
        val captures = Files.createTempDirectory(Files.createDirectories(Path.of("../tools/schema-serde-compat/.runs")), "streaming-blob-input-").toAbsolutePath()
        for ((schema, publicTypes) in listOf(false to true, true to true, true to false)) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.upload
                @smithy.protocols#rpcv2Cbor
                ${if (schema) "@aws.protocols#restJson1" else ""}
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
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schema,"publicConstrainedTypes":$publicTypes}}""").expectObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schema)
                val scope =
                    arrayOf(
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Tokio" to CargoDependency.Tokio.toType(),
                        "Bytes" to CargoDependency.Bytes.toType(),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        *RuntimeType.preludeScope,
                    )
                crate.testModule {
                    tokioTest("live_large_blob") {
                        rustTemplate(
                            """
                            use #{Tokio}::io::{AsyncReadExt, AsyncWriteExt};
                            for mode in 0..${if (schema) 3 else 2} {
                                let entered = ::std::sync::Arc::new(#{Tokio}::sync::Notify::new());
                                let handler_entered = entered.clone();
                                let completed = ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false));
                                let handler_completed = completed.clone();
                                let service = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                    .upload(move |${if (schema) "mut pending: #{Server}::schema::StreamingInput<crate::input::UploadInput>" else "input: crate::input::UploadInput"}| {
                                        let entered = handler_entered.clone();
                                        let completed = handler_completed.clone();
                                        async move {
                                            entered.notify_one();
                                            ${if (schema) {
                                """
                                            assert!(pending.is_present());
                                            assert!(matches!(pending.finish().await, #{Err}(#{Server}::schema::StreamingInputError::StreamNotFinished)));
                                            let mut count = 0;
                                            while let #{Some}(chunk) = pending.data.next().await {
                                                let chunk = chunk.unwrap();
                                                assert!(chunk.iter().all(|byte| *byte == b'x'));
                                                count += chunk.len();
                                            }
                                            assert_eq!(count, 1024 * 1024);
                                            let input = pending.finish().await.unwrap();
                                            """
                            } else {
                                """
                                            assert_eq!(input.fahad.as_ref().unwrap().as_ref(), vec![b'x'; 1024 * 1024]);
                                            """
                            }}
                                            assert_eq!(input.title.as_str(), "hello");
                                            completed.store(true, ::std::sync::atomic::Ordering::SeqCst);
                                            #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {})
                                        }
                                    }).build_unchecked();
                                let listener = #{Tokio}::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                                let address = listener.local_addr().unwrap();
                                let server = #{Tokio}::spawn(async move { crate::serve(listener, service.into_make_service()).await.unwrap(); });
                                let mut socket = #{Tokio}::net::TcpStream::connect(address).await.unwrap();
                                let headers = if mode == 2 {
                                    "POST /upload HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nTransfer-Encoding: chunked\r\nContent-Type: application/octet-stream\r\nx-title: hello\r\n\r\n"
                                } else {
                                    "POST /service/Uploads/operation/Upload HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nTransfer-Encoding: chunked\r\nContent-Type: application/cbor\r\nsmithy-protocol: rpc-v2-cbor\r\n\r\n"
                                };
                                socket.write_all(headers.as_bytes()).await.unwrap();
                                let prefix: &[u8] = match mode {
                                    0 => b"\xbf\x65title\x65hello\x65fahad\x5a\x00\x10\x00\x00",
                                    1 => b"\xbf\x65fahad\x5a\x00\x10\x00\x00",
                                    _ => b"",
                                };
                                if !prefix.is_empty() {
                                    socket.write_all(format!("{:x}\r\n", prefix.len()).as_bytes()).await.unwrap();
                                    socket.write_all(prefix).await.unwrap();
                                    socket.write_all(b"\r\n").await.unwrap();
                                }
                                if $schema {
                                    // The client has not sent blob bytes or EOF. Collection here would deadlock.
                                    let mut early = [0u8; 4096];
                                    #{Tokio}::time::timeout(::std::time::Duration::from_secs(5), async {
                                        #{Tokio}::select! {
                                            _ = entered.notified() => {},
                                            read = socket.read(&mut early) => panic!("early response in mode {mode}: {:?}", &early[..read.unwrap()]),
                                        }
                                    }).await.unwrap_or_else(|_| panic!("handler not entered in mode {mode}"));
                                }
                                let payload = vec![b'x'; 16 * 1024];
                                for _ in 0..64 {
                                    socket.write_all(b"4000\r\n").await.unwrap();
                                    socket.write_all(&payload).await.unwrap();
                                    socket.write_all(b"\r\n").await.unwrap();
                                }
                                let tail: &[u8] = match mode { 0 => b"\xff", 1 => b"\x65title\x65hello\xff", _ => b"" };
                                if !tail.is_empty() {
                                    socket.write_all(format!("{:x}\r\n", tail.len()).as_bytes()).await.unwrap();
                                    socket.write_all(tail).await.unwrap();
                                    socket.write_all(b"\r\n").await.unwrap();
                                }
                                socket.write_all(b"0\r\n\r\n").await.unwrap();
                                let mut response = #{Vec}::new();
                                #{Tokio}::time::timeout(::std::time::Duration::from_secs(5), socket.read_to_end(&mut response)).await.unwrap().unwrap();
                                ::std::fs::write(format!("$captures/$schema-$publicTypes-{mode}.http"), &response).unwrap();
                                assert!(response.starts_with(b"HTTP/1.1 200"), "{:?}", response);
                                assert!(completed.load(::std::sync::atomic::Ordering::SeqCst));
                                server.abort();
                            }
                            """,
                            *scope,
                        )
                    }
                    if (schema) {
                        tokioTest("optional_blob_and_late_constraints") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                for (wire, present, valid) in [
                                    (b"\xa1\x65title\x65hello".as_slice(), false, true),
                                    (b"\xa2\x65fahad\x40\x65title\x65hello".as_slice(), true, true),
                                    (b"\xa2\x65fahad\x41x\x65title\x61a".as_slice(), true, false),
                                    (b"\xa1\x65fahad\x41x".as_slice(), true, false),
                                ] {
                                    let called = ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false));
                                    let observed = called.clone();
                                    let service = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                        .upload(move |pending: #{Server}::schema::StreamingInput<crate::input::UploadInput>| {
                                            let observed = observed.clone();
                                            async move {
                                                assert_eq!(pending.is_present(), present);
                                                let result = pending.into_input_and_blob().await;
                                                if valid {
                                                    let (input, blob) = result.unwrap();
                                                    assert_eq!(input.title.as_str(), "hello");
                                                    assert_eq!(blob.is_some(), present);
                                                } else {
                                                    assert!(matches!(result, #{Err}(#{Server}::schema::StreamingInputError::Deserialize(
                                                        #{Server}::schema::DeserializeError::ConstraintViolation(_)
                                                    ))));
                                                }
                                                observed.store(true, ::std::sync::atomic::Ordering::SeqCst);
                                                #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {})
                                            }
                                        }).build_unchecked();
                                    let response = service.oneshot(#{Http}::Request::builder().method("POST")
                                        .uri("/service/Uploads/operation/Upload")
                                        .header("content-type", "application/cbor").header("smithy-protocol", "rpc-v2-cbor")
                                        .body(#{BodyUtil}::Full::new(#{Bytes}::Bytes::copy_from_slice(wire))).unwrap()).await.unwrap();
                                    assert_eq!(response.status(), 200);
                                    assert!(called.load(::std::sync::atomic::Ordering::SeqCst));
                                }
                                """,
                                *scope,
                            )
                        }
                    }
                }
            }
        }
    }
}
