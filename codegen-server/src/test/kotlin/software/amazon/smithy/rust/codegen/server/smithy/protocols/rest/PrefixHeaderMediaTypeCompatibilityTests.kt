/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.rest

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
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

/**
 * Sends media-typed prefix headers containing base64, quoting, empty tokens, repeated values,
 * invalid UTF-8, and malformed encodings through REST JSON and REST XML servers. It pins legacy
 * decoded values and rejection responses, then checks schema captures against them. Established
 * legacy panic cases are recorded explicitly, while the schema path is required to avoid panics.
 */
internal class PrefixHeaderMediaTypeCompatibilityTest {
    @Test
    fun `media typed prefix headers preserve legacy decoding and rejection behavior`() {
        val root = Path.of("..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/prefix-header-media-type.smithy")).asSmithyModel()
        val run = Files.createTempDirectory("prefix-header-media-type-")
        val protocols = listOf("JsonPrefixMedia", "XmlPrefixMedia")
        for (schemaSerde in listOf(false, true)) {
            for (service in protocols) {
                val mode = if (schemaSerde) "schema" else "legacy"
                val settings =
                    ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode().toBuilder()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.prefixmedia#$service",
                        additionalSettings = settings.build(),
                        overrideTestDir = run.resolve("$service-$mode").toFile(),
                        command = { path ->
                            val log = run.resolve("$service-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--quiet", "--lib", "prefix_header_media_type")
                                    .directory(path.toFile()).redirectErrorStream(true)
                            process.environment()["RUSTFLAGS"] = "-D warnings"
                            val running = process.start()
                            Files.newBufferedWriter(log).use { output ->
                                running.inputStream.bufferedReader().useLines { lines ->
                                    lines.forEach { line ->
                                        println(line)
                                        output.write(line)
                                        output.newLine()
                                        output.flush()
                                    }
                                }
                            }
                            check(running.waitFor() == 0) { "Cargo failed: $log" }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("prefix_header_media_type") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                use #{Futures}::FutureExt;
                                use #{Json}::{json, Value};
                                let app = crate::$service::builder(crate::${service}Config::builder().build())
                                    .echo(|input: crate::input::EchoInput| async move {
                                        crate::output::EchoOutput { value: Some(json!({
                                            "meta": input.meta, "direct": input.direct, "q": input.q, "params": input.params
                                        }).to_string()) }
                                    })
                                    .build().unwrap();
                                // Status 0 records a caught generated serde panic, not an HTTP response.
                                let cases: Vec<(&str, Vec<(&str, &[u8])>, u16, Option<Value>)> = vec![
                                    ("missing", vec![], 200, Some(json!({}))),
                                    ("unrelated", vec![("x-other", b"!")], 200, Some(json!({}))),
                                    ("base64", vec![("x-media-key", b"e30=")], 200, Some(json!({"key":"{}"}))),
                                    ("unicode", vec![("x-media-key", b"Y2Fmw6k=")], 200, Some(json!({"key":"caf\u{e9}"}))),
                                    ("spaces", vec![("x-media-key", b" \te30=\t ")], 200, Some(json!({"key":"{}"}))),
                                    ("quoted", vec![("x-media-key", b"\"e30=\"")], 200, Some(json!({"key":"{}"}))),
                                    ("quoted-spaces", vec![("x-media-key", b" \"e30=\" ")], 400, None),
                                    ("decoded-whitespace", vec![("x-media-key", b"IHgg")], 200, Some(json!({"key":" x "}))),
                                    ("not-json", vec![("x-media-key", b"aGVsbG8=")], 200, Some(json!({"key":"hello"}))),
                                    ("decoded-nul", vec![("x-media-key", b"AA==")], 200, Some(json!({"key":"\u{0}"}))),
                                    ("decoded-newline", vec![("x-media-key", b"YQpi")], 200, Some(json!({"key":"a\nb"}))),
                                    ("whitespace-only", vec![("x-media-key", b" \t ")], 200, Some(json!({"key":""}))),
                                    ("quoted-empty", vec![("x-media-key", b"\"\"")], 200, Some(json!({"key":""}))),
                                    ("comma-only", vec![("x-media-key", b",")], 200, Some(json!({"key":""}))),
                                    ("trailing-comma", vec![("x-media-key", b"e30=,")], 200, Some(json!({"key":"{}"}))),
                                    ("empty-then-value", vec![("x-media-key", b""), ("x-media-key", b"e30=")], 200, Some(json!({"key":"{}"}))),
                                    ("value-then-empty", vec![("x-media-key", b"e30="), ("x-media-key", b"")], 200, Some(json!({"key":"{}"}))),
                                    ("multiple-keys", vec![("x-media-one", b"e30="), ("X-Media-Two", b"Y2Fmw6k=")], 200, Some(json!({"one":"{}","two":"caf\u{e9}"}))),
                                    ("empty-key", vec![("x-media-", b"e30=")], 200, Some(json!({"":"{}"}))),
                                    ("invalid-base64", vec![("x-media-key", b"!")], 400, None),
                                    ("invalid-padding", vec![("x-media-key", b"e===")], 400, None),
                                    ("invalid-decoded-utf8", vec![("x-media-key", b"/w==")], 400, None),
                                    ("invalid-wire-utf8", vec![("x-media-key", b"\xff")], 400, None),
                                    ("multiple-tokens", vec![("x-media-key", b"e30=,e30=")], 400, None),
                                    ("repeated", vec![("x-media-key", b"e30="), ("x-media-key", b"e30=")], 400, None),
                                    ("leading-comma", vec![("x-media-key", b",e30=")], 400, None),
                                    ("double-trailing-comma", vec![("x-media-key", b"e30=,,")], 400, None),
                                    ("trailing-comma-space", vec![("x-media-key", b"e30=, ")], 400, None),
                                    ("quoted-comma", vec![("x-media-key", b"\"e30=,e30=\"")], 400, None),
                                    ("unclosed-quote", vec![("x-media-key", b"\"e30=")], 400, None),
                                    ("empty-then-invalid-wire", vec![("x-media-empty", b""), ("x-media-bad", b"\xff")], 0, None),
                                    ("invalid-wire-then-empty", vec![("x-media-bad", b"\xff"), ("x-media-empty", b"")], 400, None),
                                    ("empty-then-invalid-base64", vec![("x-media-empty", b""), ("x-media-bad", b"!")], 0, None),
                                    ("invalid-base64-then-empty", vec![("x-media-bad", b"!"), ("x-media-empty", b"")], 400, None),
                                    ("empty", vec![("x-media-key", b"")], 0, None),
                                    ("two-empty", vec![("x-media-key", b""), ("x-media-key", b"")], 0, None),
                                ];
                                let mut observations = Vec::new();
                                for (name, headers, expected_status, expected_meta) in cases {
                                    let mut request = #{Http}::Request::get("/echo?q=%20e30%3D%20&k=%21")
                                        .header("x-direct", "e30=");
                                    for (key, value) in headers {
                                        request = request.header(key, #{Http}::HeaderValue::from_bytes(value).unwrap());
                                    }
                                    let result = std::panic::AssertUnwindSafe(app.clone().oneshot(
                                        request.body(#{Server}::body::Body::empty()).unwrap())).catch_unwind().await;
                                    let response = match result {
                                        Ok(response) => response.unwrap(),
                                        Err(error) => {
                                            assert!(${!schemaSerde}, "schema deserialization must not panic: {name}");
                                            assert_eq!(expected_status, 0, "unexpected legacy panic: {name}");
                                            let message = error.downcast_ref::<&str>().copied()
                                                .or_else(|| error.downcast_ref::<String>().map(String::as_str)).unwrap_or("non-string panic");
                                            observations.push(json!({"name":name,"panic":true,"message":message}));
                                            continue;
                                        }
                                    };
                                    let status = response.status().as_u16();
                                    let mut headers = response.headers().iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.as_bytes().to_vec())
                                    ).collect::<Vec<_>>();
                                    headers.sort();
                                    let body = response.into_body().collect().await.unwrap().to_bytes();
                                    if ${!schemaSerde} {
                                        assert_eq!(status, expected_status, "legacy status: {name}");
                                        if let Some(meta) = expected_meta {
                                            assert_eq!(#{Json}::from_slice::<Value>(&body).unwrap(),
                                                json!({"meta":meta,"direct":"{}","q":" e30= ","params":{"q":" e30= ","k":"!"}}),
                                                "legacy input: {name}");
                                        }
                                    }
                                    observations.push(json!({"name":name,"panic":false,"status":status,"headers":headers,"body":body.to_vec()}));
                                }
                                if $schemaSerde {
                                    let legacy: Vec<Value> = #{Json}::from_str(
                                        &std::fs::read_to_string(#{LegacySnapshot:S}).unwrap()).unwrap();
                                    assert_eq!(observations.len(), legacy.len());
                                    let rejection = legacy.iter().find(|row| row["name"] == "repeated").unwrap();
                                    for (actual, reference) in observations.iter().zip(&legacy) {
                                        if reference["panic"] == true {
                                            // Intentional compatibility exception: reject empty values rather than panic.
                                            let mut expected = rejection.clone();
                                            expected["name"] = reference["name"].clone();
                                            assert_eq!(*actual, expected, "empty prefix header must be rejected");
                                        } else {
                                            assert_eq!(actual, reference);
                                        }
                                    }
                                }
                                std::fs::write(#{Snapshot:S}, #{Json}::to_string_pretty(&observations).unwrap()).unwrap();
                                """.trimIndent(),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Json" to CargoDependency.SerdeJson.toType(),
                                "Futures" to CargoDependency.FuturesUtil.copy(features = setOf("std", "async-await")).toType(),
                                "LegacySnapshot" to run.resolve("$service-legacy.json").toString(),
                                "Snapshot" to run.resolve("$service-$mode.json").toString(),
                            )
                        }
                    }
                }
            }
        }
    }
}

/**
 * Returns media-typed prefix header maps on successful outputs and modeled errors, including
 * empty maps, special values, and invalid header keys. It checks base64 encoding, preservation
 * of plain headers, and rejection status, then compares legacy and schema captures. This guards
 * the interaction between generated prefix-header metadata and runtime response serialization.
 */
internal class ResponsePrefixHeaderMediaTypeCompatibilityTest {
    @Test
    fun `media typed response prefix headers preserve legacy encoding and errors`() {
        val root = Path.of("..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/response-prefix-header-media-type.smithy")).asSmithyModel()
        val run =
            Files.createTempDirectory("C04-")
        val protocols = listOf("JsonResponsePrefixMedia", "XmlResponsePrefixMedia")
        for (schemaSerde in listOf(false, true)) {
            for (service in protocols) {
                val mode = if (schemaSerde) "schema" else "legacy"
                val settings =
                    ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode().toBuilder()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.responseprefixmedia#$service",
                        additionalSettings = settings.build(),
                        overrideTestDir = run.resolve("$service-$mode").toFile(),
                        command = { path ->
                            val log = run.resolve("$service-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--quiet", "--lib", "response_prefix_header_media_type")
                                    .directory(path.toFile()).redirectErrorStream(true).redirectOutput(log.toFile())
                            process.environment()["RUSTFLAGS"] = "-D warnings"
                            check(process.start().waitFor() == 0) { "Cargo failed: $log" }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("response_prefix_header_media_type") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                use #{Json}::json;
                                let cases: #{Vec}<(&str, #{Option}<#{Vec}<(&str, &str)>>)> = vec![
                                    ("missing", #{None}),
                                    ("empty-map", #{Some}(vec![])),
                                    ("json", #{Some}(vec![("key", "{}")])),
                                    ("empty-value", #{Some}(vec![("key", "")])),
                                    ("unicode", #{Some}(vec![("key", "café")])),
                                    ("spaces", #{Some}(vec![("key", " x ")])),
                                    ("tabs", #{Some}(vec![("key", "\t")])),
                                    ("newline", #{Some}(vec![("key", "a\nb")])),
                                    ("crlf", #{Some}(vec![("key", "a\r\nb")])),
                                    ("nul", #{Some}(vec![("key", "\0")])),
                                    ("comma", #{Some}(vec![("key", "a,b")])),
                                    ("quotes", #{Some}(vec![("key", "\"quoted\"")])),
                                    ("already-base64", #{Some}(vec![("key", "e30=")])),
                                    ("empty-key", #{Some}(vec![("", "{}")])),
                                    ("mixed-case-key", #{Some}(vec![("KeY", "{}")])),
                                    ("multiple", #{Some}(vec![("one", "{}"), ("two", "café"), ("empty", "")])),
                                    ("invalid-key-space", #{Some}(vec![("bad key", "{}")])),
                                    ("invalid-key-newline", #{Some}(vec![("bad\nkey", "{}")])),
                                    ("invalid-key-unicode", #{Some}(vec![("café", "{}")])),
                                ];
                                let mut observations = #{Vec}::new();
                                for (name, entries) in cases {
                                    for outcome in ["success", "error", "stream"] {
                                        let meta = entries.as_ref().map(|entries| entries.iter()
                                            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                                            .collect::<std::collections::HashMap<_, _>>());
                                        let stream_meta = meta.clone();
                                        let error_meta = meta.clone();
                                        let app = crate::$service::builder(crate::${service}Config::builder().build())
                                            .emit(move |_| {
                                                let meta = meta.clone();
                                                async move {
                                                    crate::output::EmitOutput {
                                                        meta, direct: #{Some}("{}".into()), plain: #{Some}("plain, value".into()),
                                                        body: #{Some}("response".into()),
                                                    }
                                                }
                                            })
                                            .fail(move |_| {
                                                let meta = error_meta.clone();
                                                async move {
                                                    #{Err}(crate::error::FailError::Failure(crate::error::Failure {
                                                        message: #{Some}("failure".into()), meta,
                                                        direct: #{Some}("{}".into()), plain: #{Some}("plain, value".into()),
                                                    }))
                                                }
                                            })
                                            .stream(move |_| {
                                                let meta = stream_meta.clone();
                                                async move { crate::output::StreamOutput {
                                                    meta, direct: #{Some}("{}".into()), plain: #{Some}("plain, value".into()),
                                                    body: #{Types}::byte_stream::ByteStream::from_static(b"stream\0\xff"),
                                                } }
                                            })
                                            .build().unwrap();
                                        let uri = if outcome == "stream" { "/stream" } else if outcome == "error" { "/fail" } else { "/emit" };
                                        let response = app.oneshot(#{Http}::Request::get(uri)
                                            .body(#{Server}::body::Body::empty()).unwrap()).await.unwrap();
                                        let status = response.status().as_u16();
                                        if ${!schemaSerde} {
                                            let invalid_key = name.starts_with("invalid-key");
                                            assert_eq!(status, if invalid_key { 400 } else if outcome == "error" { 422 } else { 200 }, "{name}/{outcome}");
                                            if !invalid_key {
                                                assert_eq!(response.headers()["x-direct"], "e30=");
                                                assert_eq!(response.headers()["x-plain"], "plain, value");
                                                let expected = entries.as_ref().map_or(0, #{Vec}::len);
                                                assert_eq!(response.headers().keys().filter(|k| k.as_str().starts_with("x-media-")).count(), expected);
                                                if let #{Some}(entries) = &entries {
                                                    for (key, value) in entries {
                                                        let key = format!("x-media-{}", key.to_ascii_lowercase());
                                                        assert_eq!(response.headers()[&key], #{Types}::base64::encode(value), "{name}/{outcome}");
                                                    }
                                                }
                                            }
                                        }
                                        let mut headers = response.headers().iter().map(|(name, value)|
                                            (name.as_str().to_owned(), value.as_bytes().to_vec())).collect::<#{Vec}<_>>();
                                        headers.sort();
                                        let body = response.into_body().collect().await.unwrap().to_bytes();
                                        if ${!schemaSerde} && !name.starts_with("invalid-key") {
                                            if outcome == "success" { assert_eq!(body.as_ref(), b"response"); }
                                            if outcome == "stream" { assert_eq!(body.as_ref(), b"stream\0\xff"); }
                                        }
                                        observations.push(json!({"name":name,"outcome":outcome,"status":status,"headers":headers,"body":body.to_vec()}));
                                    }
                                }
                                std::fs::write(#{Snapshot:S}, #{Json}::to_string_pretty(&observations).unwrap()).unwrap();
                                """.trimIndent(),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Json" to CargoDependency.SerdeJson.toType(),
                                "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                                *RuntimeType.preludeScope,
                                "Snapshot" to run.resolve("$service-$mode.json").toString(),
                            )
                        }
                    }
                }
            }
        }
        for (service in protocols) {
            Files.readString(run.resolve("$service-schema.json")) shouldBe Files.readString(run.resolve("$service-legacy.json"))
        }
    }
}
