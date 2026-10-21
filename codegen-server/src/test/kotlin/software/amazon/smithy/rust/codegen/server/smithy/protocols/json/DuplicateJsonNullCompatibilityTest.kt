/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.json

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

/**
 * Sends JSON objects with missing, repeated, null, and incorrectly typed members through REST
 * JSON and AWS JSON 1.0/1.1 servers. The handler exposes the final assigned scalar values so the
 * test pins legacy overwrite/null semantics and rejection behavior. It compares complete
 * response captures from independently generated legacy and schema services.
 */
internal class DuplicateJsonNullCompatibilityTest {
    @Test
    fun `duplicate JSON null values match legacy member assignment`() {
        val root = Path.of(System.getenv("SCHEMA_COMPAT_ROOT") ?: "..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server-test/schema-serde-compat/models/duplicate-json-null.smithy")).asSmithyModel()
        val run = System.getenv("SCHEMA_COMPAT_RUN")?.let(Path::of) ?: Files.createTempDirectory("duplicate-json-null-")
        val protocols = listOf("RestDuplicateNull", "Json10DuplicateNull", "Json11DuplicateNull")
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
                        service = "compat.duplicatenull#$service",
                        additionalSettings = settings.build(),
                        overrideTestDir = run.resolve("$service-$mode").toFile(),
                        command = { path ->
                            val log = run.resolve("$service-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--verbose", "--lib", "duplicate_json_null")
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
                        tokioTest("duplicate_json_null") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                use #{Json}::{json, Value};
                                let app = crate::$service::builder(crate::${service}Config::builder().build())
                                    .echo(|input: crate::input::EchoInput| async move {
                                        crate::output::EchoOutput { value: Some(json!({
                                            "text": input.text, "number": input.number
                                        }).to_string()) }
                                    }).build().unwrap();
                                let cases = [
                                    ("missing", r##"{}"##),
                                    ("null", r##"{"text":null}"##),
                                    ("text-null", r##"{"text":"a","text":null}"##),
                                    ("null-text", r##"{"text":null,"text":"b"}"##),
                                    ("text-repeated", r##"{"text":"a","text":"b"}"##),
                                    ("text-null-text", r##"{"text":"a","text":null,"text":"b"}"##),
                                    ("number-null", r##"{"number":7,"number":null}"##),
                                    ("unknown-null", r##"{"unknown":1,"unknown":null}"##),
                                    ("invalid-before-null", r##"{"text":7,"text":null}"##),
                                    ("invalid-after-null", r##"{"text":null,"text":7}"##),
                                ];
                                let mut observations = Vec::new();
                                for (name, body) in cases {
                                    let request = if ${service == "RestDuplicateNull"} {
                                        #{Http}::Request::post("/echo").header("content-type", "application/json")
                                    } else {
                                        #{Http}::Request::post("/")
                                            .header("content-type", "application/x-amz-json-${if (service == "Json10DuplicateNull") "1.0" else "1.1"}")
                                            .header("x-amz-target", "$service.Echo")
                                    };
                                    let response = app.clone().oneshot(request.body(#{Server}::body::Body::from(body)).unwrap()).await.unwrap();
                                    let status = response.status().as_u16();
                                    let mut headers = response.headers().iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.as_bytes().to_vec())).collect::<Vec<_>>();
                                    headers.sort();
                                    let body = response.into_body().collect().await.unwrap().to_bytes();
                                    if ${!schemaSerde} {
                                        if name.starts_with("invalid-") {
                                            assert_eq!(status, 400, "{name}");
                                        } else {
                                            assert_eq!(status, 200, "{name}");
                                            let value: Value = #{Json}::from_slice(&body).unwrap();
                                            let value = if ${service == "RestDuplicateNull"} { value } else {
                                                #{Json}::from_str::<Value>(value["value"].as_str().unwrap()).unwrap()
                                            };
                                            let mut expected = json!({"text":null,"number":null});
                                            if ["null-text", "text-repeated", "text-null-text"].contains(&name) {
                                                expected["text"] = json!("b");
                                            }
                                            assert_eq!(value, expected, "legacy input: {name}");
                                        }
                                    }
                                    observations.push(json!({"name":name,"status":status,"headers":headers,"body":body.to_vec()}));
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
