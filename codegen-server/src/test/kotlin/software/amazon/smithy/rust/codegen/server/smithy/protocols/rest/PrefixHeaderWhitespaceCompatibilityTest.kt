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
 * Sends plain-string and enum prefix headers with surrounding whitespace, empty values, and
 * invalid enum values to legacy and schema REST services. Handler output exposes the parsed map
 * values so the test can pin legacy trimming and validation rules. Status, headers, and response
 * bodies must agree between the two generated implementations.
 */
internal class PrefixHeaderWhitespaceCompatibilityTest {
    @Test
    fun `prefix header strings preserve legacy whitespace and validation semantics`() {
        val root = Path.of("..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/prefix-header-whitespace.smithy")).asSmithyModel()
        val run = Files.createTempDirectory("prefix-header-whitespace-")
        val protocols = listOf("JsonPrefixHeaders", "XmlPrefixHeaders")
        for (schemaSerde in listOf(false, true)) {
            for (service in protocols) {
                val mode = if (schemaSerde) "schema" else "legacy"
                val settings =
                    ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode().toBuilder()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.prefixheaders#$service",
                        additionalSettings = settings.build(),
                        overrideTestDir = run.resolve("$service-$mode").toFile(),
                        command = { path ->
                            val log = run.resolve("$service-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--quiet", "--lib", "prefix_header_whitespace")
                                    .directory(path.toFile()).redirectErrorStream(true).redirectOutput(log.toFile())
                            process.environment()["RUSTFLAGS"] = "-D warnings"
                            check(process.start().waitFor() == 0) { "Cargo failed: $log" }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("prefix_header_whitespace") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                use #{Json}::{json, Value};
                                let app = crate::$service::builder(crate::${service}Config::builder().build())
                                    .echo(|input: crate::input::EchoInput| async move {
                                        crate::output::EchoOutput { value: Some(json!({
                                            "meta": input.meta, "direct": input.direct, "q": input.q, "params": input.params
                                        }).to_string()) }
                                    })
                                    .echo_enum(|input: crate::input::EchoEnumInput| async move {
                                        let meta = input.meta.map(|map| map.into_iter().map(|(k, v)|
                                            (k, v.as_str().to_owned())).collect::<std::collections::BTreeMap<_, _>>());
                                        Ok::<_, crate::error::EchoEnumError>(crate::output::EchoEnumOutput { value: Some(json!({"meta": meta}).to_string()) })
                                    })
                                    .build().unwrap();
                                let cases: Vec<(&str, Vec<(&str, &[u8])>, u16, Option<Value>)> = vec![
                                    ("missing", vec![], 200, Some(json!({}))),
                                    ("unrelated", vec![("x-other", b" ignored ")], 200, Some(json!({}))),
                                    ("plain", vec![("x-meta-key", b"red")], 200, Some(json!({"key":"red"}))),
                                    ("spaces", vec![("x-meta-key", b"  red  ")], 200, Some(json!({"key":"red"}))),
                                    ("leading-tab", vec![("x-meta-key", b"\tred")], 200, Some(json!({"key":"red"}))),
                                    ("trailing-tab", vec![("x-meta-key", b"red\t")], 200, Some(json!({"key":"red"}))),
                                    ("mixed-whitespace", vec![("x-meta-key", b" \t red \t ")], 200, Some(json!({"key":"red"}))),
                                    ("empty", vec![("x-meta-key", b"")], 200, Some(json!({"key":""}))),
                                    ("spaces-only", vec![("x-meta-key", b"   ")], 200, Some(json!({"key":""}))),
                                    ("tabs-only", vec![("x-meta-key", b" \t\t ")], 200, Some(json!({"key":""}))),
                                    ("interior", vec![("x-meta-key", b" red \t blue ")], 200, Some(json!({"key":"red \t blue"}))),
                                    ("commas", vec![("x-meta-key", b" red, blue ")], 200, Some(json!({"key":"red, blue"}))),
                                    ("quotes", vec![("x-meta-key", b" \" red, blue \" ")], 200, Some(json!({"key":"\" red, blue \""}))),
                                    ("unmatched-quote", vec![("x-meta-key", b" \"red ")], 200, Some(json!({"key":"\"red"}))),
                                    ("unicode-whitespace", vec![("x-meta-key", "\u{a0}red\u{2003}".as_bytes())], 200, Some(json!({"key":"red"}))),
                                    ("unicode-text", vec![("x-meta-key", " café ".as_bytes())], 200, Some(json!({"key":"café"}))),
                                    ("zero-width-space", vec![("x-meta-key", " \u{200b}red\u{200b} ".as_bytes())], 200, Some(json!({"key":"\u{200b}red\u{200b}"}))),
                                    ("multiple-keys", vec![("x-meta-red", b" one "), ("X-Meta-Blue", b"\ttwo\t")], 200, Some(json!({"red":"one","blue":"two"}))),
                                    ("empty-key", vec![("x-meta-", b" value ")], 200, Some(json!({"":"value"}))),
                                    ("repeated", vec![("x-meta-key", b" red "), ("x-meta-key", b" blue ")], 400, None),
                                    ("repeated-empty", vec![("x-meta-key", b""), ("x-meta-key", b" ")], 400, None),
                                    ("invalid-utf8", vec![("x-meta-key", b"\xff")], 400, None),
                                ];
                                let mut requests = Vec::new();
                                for (name, headers, status, meta) in cases {
                                    let expected = meta.map(|meta| json!({"meta":meta, "direct":"direct, \"quoted\"",
                                        "q":" query ", "params":{"q":" query ", "k":"\tparams\t"}}));
                                    requests.push((name, "/echo?q=%20query%20&k=%09params%09", headers, status, expected));
                                }
                                for (name, raw, value) in [
                                    ("enum-plain", "red", "red"),
                                    ("enum-spaces", " red ", "red"),
                                    ("enum-tabs", "\tblue\t", "blue"),
                                    ("enum-unicode-whitespace", "\u{a0}red\u{2003}", "red"),
                                ] {
                                    requests.push((name, "/enum", vec![("x-color-key", raw.as_bytes())], 200,
                                        Some(json!({"meta":{"key":value}}))));
                                }
                                let mut observations = Vec::new();
                                for (name, uri, headers, expected_status, expected_body) in requests {
                                    let mut request = #{Http}::Request::get(uri).header("x-direct", " \t direct, \"quoted\" \t ");
                                    for (key, value) in headers {
                                        request = request.header(key, #{Http}::HeaderValue::from_bytes(value).unwrap());
                                    }
                                    let response = app.clone().oneshot(request.body(#{Server}::body::Body::empty()).unwrap()).await.unwrap();
                                    let status = response.status().as_u16();
                                    let mut headers = response.headers().iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.as_bytes().to_vec())
                                    ).collect::<Vec<_>>();
                                    headers.sort();
                                    let body = response.into_body().collect().await.unwrap().to_bytes();
                                    if ${!schemaSerde} {
                                        assert_eq!(status, expected_status, "legacy status: {name}");
                                        if let Some(expected) = expected_body {
                                            assert_eq!(#{Json}::from_slice::<Value>(&body).unwrap(), expected, "legacy input: {name}");
                                        }
                                    }
                                    observations.push(json!({"name":name, "status":status, "headers":headers, "body":body.to_vec()}));
                                }
                                std::fs::write(#{Snapshot:S}, #{Json}::to_string_pretty(&observations).unwrap()).unwrap();
                                """.trimIndent(),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Json" to CargoDependency.SerdeJson.toType(),
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
