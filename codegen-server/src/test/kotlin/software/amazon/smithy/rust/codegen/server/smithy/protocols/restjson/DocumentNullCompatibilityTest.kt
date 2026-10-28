/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.restjson

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
 * Sends absent bodies, explicit JSON null, ordinary document values, and malformed JSON to
 * legacy and schema REST JSON payload handlers. It checks that null remains a present Document
 * value while an absent body remains absent. Structured and union payloads must retain the
 * legacy rejection of null. Complete recorded responses must agree between the two serde modes.
 */
internal class DocumentNullCompatibilityTest {
    @Test
    fun `REST JSON document payload null remains present like legacy`() {
        val root = Path.of("..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/document-null.smithy")).asSmithyModel()
        val run = Files.createTempDirectory("document-null-")
        val protocols = listOf("RestDocumentNull")
        for (schemaSerde in listOf(false, true)) {
            for (service in protocols) {
                val mode =
                    if (schemaSerde) {
                        "schema"
                    } else {
                        "legacy"
                    }
                val settings =
                    ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode().toBuilder()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        service = "compat.documentnull#$service",
                        additionalSettings = settings.build(),
                        overrideTestDir = run.resolve("$service-$mode").toFile(),
                        command = { path ->
                            val log = run.resolve("$service-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--verbose", "--lib", "document_null")
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
                        tokioTest("document_null") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                use #{Json}::{json, Value};
                                let app = crate::$service::builder(crate::${service}Config::builder().build())
                                    .structure_payload(|_: crate::input::StructurePayloadInput| async move { crate::output::StructurePayloadOutput {} })
                                    .union_payload(|_: crate::input::UnionPayloadInput| async move { crate::output::UnionPayloadOutput {} })
                                    .payload(|input: crate::input::PayloadInput| async move {
                                        crate::output::PayloadOutput { value: Some(json!({
                                            "present": input.document.is_some(),
                                            "null": input.document.as_ref().is_some_and(|v| v.is_null())
                                        }).to_string()) }
                                    }).build().unwrap();
                                let payload_cases = [
                                    ("missing", "", false, false),
                                    ("null", "null", true, true),
                                    ("whitespace-null", " \n null \t ", true, true),
                                    ("boolean", "true", true, false),
                                    ("nested", r##"{"a":[null,{"b":null}]}"##, true, false),
                                    ("object", r##"{"key":null}"##, true, false),
                                    ("string", r##""a""##, true, false),
                                    ("number", "7", true, false),
                                    ("array", "[null]", true, false),
                                    ("invalid", "nul", false, false),
                                ];
                                let mut observations = Vec::new();
                                for operation in ["Payload", "StructurePayload", "UnionPayload"] {
                                    for &(name, body, present, expected_null) in &payload_cases {
                                        if operation != "Payload" && !matches!(name, "null" | "whitespace-null") { continue; }
                                        let request = #{Http}::Request::post(format!("/{}", operation.to_lowercase()))
                                            .header("content-type", "application/json");
                                        let response = app.clone().oneshot(request.body(#{Server}::body::Body::from(body)).unwrap()).await.unwrap();
                                        let status = response.status().as_u16();
                                        let mut headers = response.headers().iter().map(|(name, value)|
                                            (name.as_str().to_owned(), value.as_bytes().to_vec())).collect::<Vec<_>>();
                                        headers.sort();
                                        let body = response.into_body().collect().await.unwrap().to_bytes();
                                        if ${!schemaSerde} {
                                            if name == "invalid" || operation != "Payload" {
                                                assert_eq!(status, 400, "{operation}/{name}");
                                            } else {
                                                assert_eq!(status, 200, "{operation}/{name}");
                                                let value: Value = #{Json}::from_slice(&body).unwrap();
                                                assert_eq!(value, json!({"present":present,"null":expected_null}), "legacy: {operation}/{name}");
                                            }
                                        }
                                        observations.push(json!({"name":format!("{operation}/{name}"),"status":status,"headers":headers,"body":body.to_vec()}));
                                    }
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
            check(Files.readString(run.resolve("$service-legacy.json")) == Files.readString(run.resolve("$service-schema.json"))) {
                "Legacy/schema discrepancy for $service; observations: $run"
            }
        }
    }
}
