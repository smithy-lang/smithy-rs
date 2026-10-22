/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.streaming

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
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

/**
 * Drives generated servers over sockets with public and private constrained input types.
 * REST handlers must receive a live streaming blob before upload EOF, while CBOR handlers receive
 * the collected blob representation. Large and optional uploads check full byte consumption,
 * header validation, and completion so input builder changes cannot accidentally buffer REST streams.
 */
internal class StreamingBlobInputTest {
    @Test
    fun `REST inputs remain live and CBOR inputs are collected with public and private builders`() {
        val captures = Files.createTempDirectory("streaming-blob-input-").toAbsolutePath()
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
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schema).publicConstrainedTypes(publicTypes).toObjectNode(),
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
                            for mode in ${if (schema) "0..3" else "0..2"} {
                                let entered = ::std::sync::Arc::new(#{Tokio}::sync::Notify::new());
                                let handler_entered = entered.clone();
                                let completed = ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false));
                                let handler_completed = completed.clone();
                                let service = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                    .upload(move |input: crate::input::UploadInput| {
                                        let entered = handler_entered.clone();
                                        let completed = handler_completed.clone();
                                        async move {
                                            entered.notify_one();
                                            ${if (schema) {
                                """
                                            assert_eq!(input.title.as_str(), "hello");
                                            let mut stream = input.fahad;
                                            let mut count = 0;
                                            while let #{Some}(chunk) = stream.next().await {
                                                let chunk = chunk.unwrap();
                                                assert!(chunk.iter().all(|byte| *byte == b'x'));
                                                count += chunk.len();
                                            }
                                            assert_eq!(count, 1024 * 1024);
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
                                if mode == 2 {
                                    // The client has not sent blob bytes or EOF. Collection here would deadlock.
                                    let mut early = [0u8; 4096];
                                    #{Tokio}::time::timeout(::std::time::Duration::from_secs(5), async {
                                        #{Tokio}::select! {
                                            _ = entered.notified() => {},
                                            read = socket.read(&mut early) => panic!("early response in mode {mode}: {:?}", &early[..read.unwrap()]),
                                        }
                                    }).await.unwrap_or_else(|_| panic!("handler not entered in mode {mode}"));
                                } else {
                                    // Neither the blob nor trailing fields have arrived yet.
                                    // CBOR handlers must wait for the complete validated input.
                                    assert!(#{Tokio}::time::timeout(
                                        ::std::time::Duration::from_millis(100), entered.notified()
                                    ).await.is_err(), "CBOR handler entered before body completion");
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
                    run {
                        val uploadBytes =
                            if (schema) {
                                "input.fahad.collect().await.unwrap().into_bytes().to_vec()"
                            } else {
                                "input.fahad.as_ref().map(|blob| blob.as_ref().to_vec()).unwrap_or_default()"
                            }
                        val optionalBytes =
                            if (schema) {
                                "input.data.collect().await.unwrap().into_bytes().to_vec()"
                            } else {
                                "input.data.as_ref().map(|blob| blob.as_ref().to_vec()).unwrap_or_default()"
                            }
                        tokioTest("cbor_blob_inputs_match_legacy_before_dispatch") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                let mut records = #{String}::new();
                                for wire in [
                                    b"\xa2\x65title\x65hello\x65fahad\x41x".as_slice(),
                                    b"\xa2\x65fahad\x41x\x65title\x65hello".as_slice(),
                                    b"\xa2\x65fahad\x41x\x65title\x61a".as_slice(),
                                    b"\xa1\x65fahad\x41x".as_slice(),
                                    b"\xa3\x65fahad\x41x\x65title\x65hello\x65fahad\x41y".as_slice(),
                                    b"\xa2\x65fahad\x41x\x65title\x65hello\x00".as_slice(),
                                    b"\xa0".as_slice(),
                                    b"".as_slice(),
                                    b"\xa1\x65title\x65hello".as_slice(),
                                    b"\xa2\x65title\x65hello\x65fahad\x40".as_slice(),
                                    b"\xa2\x65title\x65hello\x65fahad\xf6".as_slice(),
                                    b"\xa2\x65title\x65hello\x65fahad\x43x".as_slice(),
                                    b"\xa2\x65title\x65hello\x65fahad\x5f\x41x\x41y\xff".as_slice(),
                                    b"\xa1\x64data\x41x".as_slice(),
                                    b"\xa1\x64data\x40".as_slice(),
                                    b"\xa1\x64data\xf6".as_slice(),
                                ] {
                                    for operation in ["Upload", "OptionalOnly"] {
                                        let observed = ::std::sync::Arc::new(::std::sync::Mutex::new(#{None}));
                                        let upload_observed = observed.clone();
                                        let optional_observed = observed.clone();
                                        let eof = ::std::sync::Arc::new(::std::sync::atomic::AtomicBool::new(false));
                                        let upload_eof = eof.clone();
                                        let optional_eof = eof.clone();
                                        let service = crate::Uploads::builder(crate::UploadsConfig::builder().build())
                                            .upload(move |input: crate::input::UploadInput| {
                                                assert!(upload_eof.load(::std::sync::atomic::Ordering::SeqCst));
                                                let observed = upload_observed.clone();
                                                async move {
                                                    let bytes = $uploadBytes;
                                                    *observed.lock().unwrap() = #{Some}((input.title.as_str().to_owned(), bytes));
                                                    #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {})
                                                }
                                            })
                                            .optional_only(move |input: crate::input::OptionalOnlyInput| {
                                                assert!(optional_eof.load(::std::sync::atomic::Ordering::SeqCst));
                                                let observed = optional_observed.clone();
                                                async move {
                                                    let bytes = $optionalBytes;
                                                    *observed.lock().unwrap() = #{Some}((#{String}::new(), bytes));
                                                    crate::output::OptionalOnlyOutput {}
                                                }
                                            }).build_unchecked();
                                        let mut bytes = wire.to_vec().into_iter();
                                        let body_eof = eof.clone();
                                        let frames = #{Futures}::stream::poll_fn(move |_| {
                                            let next = bytes.next().map(|byte|
                                                #{Ok}::<_, ::std::convert::Infallible>(#{HttpBody}::Frame::data(#{Bytes}::Bytes::from(vec![byte])))
                                            );
                                            if next.is_none() {
                                                body_eof.store(true, ::std::sync::atomic::Ordering::SeqCst);
                                            }
                                            ::std::task::Poll::Ready(next)
                                        });
                                        let response = service.oneshot(#{Http}::Request::builder().method("POST")
                                            .uri(format!("/service/Uploads/operation/{operation}"))
                                            .header("content-type", "application/cbor")
                                            .header("smithy-protocol", "rpc-v2-cbor")
                                            .body(#{BodyUtil}::StreamBody::new(frames)).unwrap()).await.unwrap();
                                        let status = response.status().as_u16();
                                        let mut headers = response.headers().iter().map(|(name, value)|
                                            (name.as_str().to_owned(), value.to_str().unwrap().to_owned())
                                        ).collect::<#{Vec}<_>>();
                                        headers.sort();
                                        let body = response.into_body().collect().await.unwrap().to_bytes();
                                        let value: #{Ciborium}::value::Value = #{Ciborium}::de::from_reader(body.as_ref()).unwrap();
                                        assert_eq!(observed.lock().unwrap().is_some(), status == 200);
                                        records.push_str(&format!("{operation}\t{:02x?}\t{status}\t{headers:?}\t{value:?}\t{:?}\n",
                                            wire, *observed.lock().unwrap()));
                                    }
                                }
                                ::std::fs::write("$captures/$schema-$publicTypes-inputs.txt", records).unwrap();
                                """,
                                *scope,
                                "Futures" to CargoDependency.FuturesUtil.toType(),
                                "HttpBody" to CargoDependency.HttpBody1x.toType(),
                                "Ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
                            )
                        }
                    }
                }
            }
        }
        val legacy = Files.readAllLines(captures.resolve("false-true-inputs.txt"))
        for (publicTypes in listOf(true, false)) {
            Files.readAllLines(captures.resolve("true-$publicTypes-inputs.txt")) shouldBe legacy
        }
    }
}
