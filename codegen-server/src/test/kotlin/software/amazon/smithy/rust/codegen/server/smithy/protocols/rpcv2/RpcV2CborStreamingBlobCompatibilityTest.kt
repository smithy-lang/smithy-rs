/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.rpcv2

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
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Exercises CBOR blob uploads, downloads, and event-stream responses in legacy, schema-only,
 * and shared REST/CBOR services. It compares decoded values and HTTP metadata while explicitly
 * checking schema live-blob framing and its absent content length. A separate REST upload/download
 * case checks that raw blob bytes retain legacy behavior when the generated types are shared.
 */
internal class RpcV2CborStreamingBlobCompatibilityTest {
    @Test
    fun `CBOR blob inputs and decoded outputs match legacy`() {
        val modelText =
            """
            ${'$'}version: "2"
            namespace test.streamingblob
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor
            service Downloads { version: "1", operations: [Download, Upload, Events] }
            @http(method: "POST", uri: "/download")
            operation Download {
                input := { @httpHeader("x-kind") kind: String }
                output := { @required @httpPayload data: StreamingBlob }
            }
            @http(method: "POST", uri: "/upload")
            operation Upload {
                input := { @required @httpPayload data: StreamingBlob }
                output := { data: Blob }
                errors: [smithy.framework#ValidationException]
            }
            @streaming blob StreamingBlob
            @http(method: "POST", uri: "/events")
            operation Events { input := {}, output := { @httpPayload events: EventStream } }
            @streaming union EventStream { item: Item }
            structure Item { value: String }
            """.trimIndent()
        val snapshots = Files.createTempDirectory("cbor-streaming-blob-")
        for ((schemaSerde, shared) in listOf(false to false, true to false, true to true)) {
            val model =
                (
                    if (shared) {
                        modelText.replace("@rpcv2Cbor", "@rpcv2Cbor @aws.protocols#restJson1")
                    } else {
                        modelText
                    }
                )
                    .asSmithyModel()
            val snapshot =
                snapshots.resolve(
                    if (shared) {
                        "shared.txt"
                    } else {
                        "$schemaSerde.txt"
                    },
                )
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                    command = { testDir ->
                        "cargo test --quiet --lib --tests".runCommand(testDir)
                        if (schemaSerde) {
                            "cargo clippy --quiet --lib -- -D clippy::manual_map".runCommand(testDir)
                        }
                    },
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                val data =
                    if (schemaSerde) {
                        """
                        #{Types}::byte_stream::ByteStream::new(#{Types}::body::SdkBody::from_body_1_x(
                            #{BodyUtil}::StreamBody::new(#{Futures}::stream::iter([
                                #{Ok}::<_, ::std::io::Error>(#{HttpBody}::Frame::data(#{Bytes}::Bytes::from(bytes))),
                                #{Ok}(#{HttpBody}::Frame::data(#{Bytes}::Bytes::new())),
                            ]))
                        ))
                        """.trimIndent()
                    } else {
                        "#{Types}::Blob::new(bytes)"
                    }
                val uploaded =
                    if (schemaSerde) {
                        """
                        input.data.collect().await.unwrap().into_bytes()
                        """.trimIndent()
                    } else {
                        "#{Bytes}::Bytes::copy_from_slice(input.data.as_ref())"
                    }
                crate.testModule {
                    tokioTest("record_streaming_responses") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            let service = crate::Downloads::builder(crate::DownloadsConfig::builder().build())
                                .download(|input: crate::input::DownloadInput| async move {
                                    let bytes = match input.kind.as_deref() {
                                        #{Some}("empty") => #{Vec}::new(),
                                        #{Some}("binary") => vec![0, 255, 1],
                                        _ => b"raw blob bytes".to_vec(),
                                    };
                                    crate::output::DownloadOutput {
                                        data: $data,
                                    }
                                })
                                .upload(move |input: #{UploadInput}| {
                                    async move {
                                    let bytes = $uploaded;
                                    #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {
                                        data: #{Some}(#{Types}::Blob::new(bytes)),
                                    })
                                }})
                                .events(|_: crate::input::EventsInput| async {
                                    crate::output::EventsOutput {
                                        events: #{Futures}::stream::iter([
                                            #{Ok}::<_, #{SmithyHttp}::event_stream::MessageStreamError>(
                                                crate::model::EventStream::Item(crate::model::Item {
                                                    value: #{Some}("value".into()),
                                                })
                                            ),
                                        ]).into(),
                                    }
                                })
                                .build().unwrap();
                            let mut records = #{String}::new();
                            let mut wire_records = #{String}::new();
                            for (operation, case, request_body) in [
                                ("Download", "raw", vec![0xa1, 0x64, b'k', b'i', b'n', b'd', 0x63, b'r', b'a', b'w']),
                                ("Download", "empty", vec![0xa1, 0x64, b'k', b'i', b'n', b'd', 0x65, b'e', b'm', b'p', b't', b'y']),
                                ("Download", "binary", vec![0xa1, 0x64, b'k', b'i', b'n', b'd', 0x66, b'b', b'i', b'n', b'a', b'r', b'y']),
                                ("Upload", "raw", [vec![0xa1, 0x64, b'd', b'a', b't', b'a', 0x4e], b"raw blob bytes".to_vec()].concat()),
                                ("Upload", "empty", vec![0xa1, 0x64, b'd', b'a', b't', b'a', 0x40]),
                                ("Upload", "binary", vec![0xa1, 0x64, b'd', b'a', b't', b'a', 0x43, 0, 255, 1]),
                                ("Upload", "duplicate", vec![0xa2, 0x64, b'd', b'a', b't', b'a', 0x41, 1, 0x64, b'd', b'a', b't', b'a', 0x41, 2]),
                                ("Upload", "trailing", vec![0xa1, 0x64, b'd', b'a', b't', b'a', 0x41, 1, 0]),
                                ("Upload", "missing", vec![0xa0]),
                                ("Upload", "malformed", vec![0xa1, 0x64, b'd', b'a', b't', b'a', 0x43, 0]),
                                ("Events", "frames", vec![]),
                            ] {
                                for accept in [#{None}, #{Some}("application/cbor"),
                                    #{Some}("application/vnd.amazon.eventstream"), #{Some}("application/json")] {
                                    let mut builder = #{Http}::Request::builder().method("POST")
                                        .uri(format!("/service/Downloads/operation/{operation}"))
                                        .header("smithy-protocol", "rpc-v2-cbor")
                                        .header("content-type", "application/cbor");
                                    if let #{Some}(accept) = accept {
                                        builder = builder.header("accept", accept);
                                    }
                                    let response = service.clone().oneshot(
                                        builder.body(#{BodyUtil}::Full::new(#{Bytes}::Bytes::from(request_body.clone()))).unwrap()
                                    ).await.unwrap();
                                    let status = response.status().as_u16();
                                    let mut headers = response.headers().iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.to_str().unwrap().to_owned())
                                    ).collect::<#{Vec}<_>>();
                                    headers.sort();
                                    let body = #{Server}::body::__private::collect_body(
                                        response.into_body(), #{Some}(#{Server}::body::__private::CollectionConfig::builder()
                                            .limit(::std::num::NonZeroUsize::new(4096).unwrap()).build()),
                                    )
                                        .await.unwrap();
                                    if let #{Some}((_, length)) = headers.iter().find(|(name, _)| name == "content-length") {
                                        assert_eq!(length.parse::<usize>().unwrap(), body.len());
                                    }
                                    if operation != "Events" && status == 200 {
                                        if operation == "Download" && $schemaSerde {
                                            assert!(!headers.iter().any(|(name, _)| name == "content-length"));
                                            assert!(body.starts_with(&[0xbf, 0x64, b'd', b'a', b't', b'a', 0x5f]));
                                            assert!(body.ends_with(&[0xff, 0xff]));
                                        } else {
                                            assert!(headers.iter().any(|(name, _)| name == "content-length"));
                                        }
                                        assert_eq!(headers.iter().find(|(name, _)| name == "content-type").unwrap().1, "application/cbor");
                                    }
                                    wire_records.push_str(&format!("{operation}\t{case}\t{accept:?}\t{status}\t{headers:?}\t{:02x?}\n", body.as_ref()));
                                    // Compare CBOR values, preserving byte strings; map framing can differ.
                                    let document = if operation == "Events" && status == 200 {
                                        body.iter().map(|byte| format!("{byte:02x}")).collect::<#{String}>()
                                    } else {
                                        let mut cursor = ::std::io::Cursor::new(&body);
                                        let value: #{Ciborium}::value::Value = #{Ciborium}::de::from_reader(&mut cursor).unwrap();
                                        assert_eq!(cursor.position() as usize, body.len());
                                        format!("{value:?}")
                                    };
                                    headers.retain(|(name, _)| name != "content-length");
                                    records.push_str(&format!("{operation}\t{case}\t{accept:?}\t{status}\t{headers:?}\t{document}\n"));
                                }
                            }
                            ::std::fs::write("${snapshot.toAbsolutePath()}", records).unwrap();
                            ::std::fs::write("${snapshot.toAbsolutePath()}.wire", wire_records).unwrap();
                            """,
                            "UploadInput" to
                                software.amazon.smithy.rust.codegen.core.rustlang.writable {
                                    rustTemplate("crate::input::UploadInput")
                                },
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "SmithyHttp" to RuntimeType.smithyHttp(context.runtimeConfig),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "HttpBody" to CargoDependency.HttpBody1x.toType(),
                            "Bytes" to CargoDependency.Bytes.toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Futures" to CargoDependency.FuturesUtil.toType(),
                            "Ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
        val legacy = Files.readAllLines(snapshots.resolve("false.txt"))
        for (mode in listOf("true", "shared")) {
            val schema = Files.readAllLines(snapshots.resolve("$mode.txt"))
            // Inputs preserve generated serde parsing, validation, duplicate-member and Accept behavior.
            // Downloads preserve decoded values while explicitly asserting streaming wire framing.
            schema shouldBe legacy
        }
    }

    @Test
    fun `REST raw blob requests and responses retain legacy behavior in a shared service`() {
        val snapshots = Files.createTempDirectory("rest-streaming-blob-")
        for ((schemaSerde, shared) in listOf(false to false, true to false, true to true)) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.streamingblob
                @aws.protocols#restJson1
                ${if (shared) {
                    "@smithy.protocols#rpcv2Cbor"
                } else {
                    ""
                }}
                service Downloads { version: "1", operations: [Download, Upload] }
                @http(method: "POST", uri: "/download")
                operation Download {
                    input := { @httpHeader("x-kind") kind: String }
                    output := { @required @httpPayload data: StreamingBlob }
                }
                @http(method: "POST", uri: "/upload")
                operation Upload {
                    input := { @required @httpPayload data: StreamingBlob }
                    output := { data: Blob }
                    errors: [smithy.framework#ValidationException]
                }
                @streaming blob StreamingBlob
                """.asSmithyModel()
            val snapshot =
                snapshots.resolve(
                    if (shared) {
                        "shared.txt"
                    } else {
                        "$schemaSerde.txt"
                    },
                )
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                crate.testModule {
                    tokioTest("rest_blobs_match_legacy") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            let service = crate::Downloads::builder(crate::DownloadsConfig::builder().build())
                                .download(|input: crate::input::DownloadInput| async move {
                                    let bytes = match input.kind.as_deref() {
                                        #{Some}("empty") => #{Vec}::new(),
                                        #{Some}("binary") => vec![0, 255, 1],
                                        _ => b"raw blob bytes".to_vec(),
                                    };
                                    crate::output::DownloadOutput { data: #{Types}::byte_stream::ByteStream::from(bytes) }
                                })
                                .upload(|input: crate::input::UploadInput| async move {
                                    let bytes = input.data.collect().await.unwrap().into_bytes().to_vec();
                                    #{Ok}::<_, crate::error::UploadError>(crate::output::UploadOutput {
                                        data: #{Some}(#{Types}::Blob::new(bytes)),
                                    })
                                })
                                .build().unwrap();
                            let mut records = #{String}::new();
                            for (kind, bytes) in [("raw", b"raw blob bytes".to_vec()),
                                ("empty", vec![]), ("binary", vec![0, 255, 1])] {
                                for operation in ["download", "upload"] {
                                    let request_body = if operation == "upload" { bytes.clone() } else { vec![] };
                                    let response = service.clone().oneshot(#{Http}::Request::builder()
                                        .method("POST").uri(format!("/{operation}"))
                                        .header("x-kind", kind)
                                        .header("content-type", if operation == "upload" { "application/octet-stream" } else { "application/json" })
                                        .body(#{BodyUtil}::Full::new(#{Bytes}::Bytes::from(request_body))).unwrap()
                                    ).await.unwrap();
                                    assert_eq!(response.status(), #{Http}::StatusCode::OK);
                                    let mut headers = response.headers().iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.to_str().unwrap().to_owned())
                                    ).collect::<#{Vec}<_>>();
                                    headers.sort();
                                    let body = #{Server}::body::__private::collect_body(
                                        response.into_body(), #{Some}(#{Server}::body::__private::CollectionConfig::builder()
                                            .limit(::std::num::NonZeroUsize::new(4096).unwrap()).build()),
                                    )
                                        .await.unwrap();
                                    if operation == "download" { assert_eq!(body.as_ref(), bytes); }
                                    let hex = body.iter().map(|byte| format!("{byte:02x}")).collect::<#{String}>();
                                    records.push_str(&format!("{operation}\t{kind}\t{headers:?}\t{hex}\n"));
                                }
                            }
                            ::std::fs::write("${snapshot.toAbsolutePath()}", records).unwrap();
                            """,
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Bytes" to CargoDependency.Bytes.toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            *RuntimeType.preludeScope,
                        )
                    }
                }
            }
        }
        Files.readString(snapshots.resolve("true.txt")) shouldBe Files.readString(snapshots.resolve("false.txt"))
        Files.readString(snapshots.resolve("shared.txt")) shouldBe Files.readString(snapshots.resolve("false.txt"))
    }
}
