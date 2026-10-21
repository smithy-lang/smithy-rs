/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.Node
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

internal class ResponsePrefixHeaderMediaTypeCompatibilityTest {
    @Test
    fun `media typed response prefix headers preserve legacy encoding and errors`() {
        val root = Path.of(System.getenv("SCHEMA_COMPAT_ROOT") ?: "..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("tools/schema-serde-compat/models/response-prefix-header-media-type.smithy")).asSmithyModel()
        val run =
            System.getenv("SCHEMA_COMPAT_RUN")?.let(Path::of) ?: Files.createDirectories(root.resolve("tools/schema-serde-compat/.runs")).let {
                Files.createTempDirectory(it, "C04-")
            }
        val protocols = listOf("JsonResponsePrefixMedia", "XmlResponsePrefixMedia")
        for (schemaSerde in listOf(false, true)) {
            for (service in protocols) {
                val mode = if (schemaSerde) "schema" else "legacy"
                val settings =
                    ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode().toBuilder()
                System.getenv("SCHEMA_COMPAT_RUNTIME")?.let {
                    settings.withMember("runtimeConfig", Node.objectNodeBuilder().withMember("relativePath", it).build())
                }
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
