/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.json

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Tag
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

/**
 * Sends JSON objects with missing, repeated, null, and incorrectly typed members through REST
 * JSON and AWS JSON 1.0/1.1 servers. The handler exposes the final assigned scalar values so the
 * test pins legacy overwrite/null semantics and rejection behavior. It compares complete
 * response captures from independently generated legacy and schema services.
 */
@Tag("server-compatibility")
internal class DuplicateJsonNullCompatibilityTest {
    @Test
    fun `duplicate JSON null values match legacy member assignment`() {
        val root = Path.of("..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/duplicate-json-null.smithy")).asSmithyModel()
        val run = Files.createTempDirectory("duplicate-json-null-")
        val protocols = listOf("RestDuplicateNull", "Json10DuplicateNull", "Json11DuplicateNull")
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
                                            .header("content-type", "application/x-amz-json-${if (service == "Json10DuplicateNull") {
                                    "1.0"
                                } else {
                                    "1.1"
                                }}")
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

/**
 * Sends explicit JSON null values at the document root and inside modeled inputs through REST
 * JSON and AWS JSON services. Generated handlers record the Document values they receive, and
 * Rust assertions distinguish present null from an omitted optional member. The same cases run
 * with legacy and schema serde and must retain their successful responses and input values.
 */
@Tag("server-compatibility")
internal class JsonDocumentNullInputCompatibilityTest {
    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "awsJson1_0", "awsJson1_1"])
    fun `legacy handlers receive explicit document null`(protocol: String) = checkInputs(protocol, schemaSerde = false)

    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "awsJson1_0", "awsJson1_1"])
    fun `schema handler inputs match legacy document null`(protocol: String) = checkInputs(protocol, schemaSerde = true)

    private fun checkInputs(
        protocol: String,
        schemaSerde: Boolean,
    ) {
        val contentType =
            when (protocol) {
                "restJson1" -> "application/json"
                "awsJson1_0" -> "application/x-amz-json-1.0"
                else -> "application/x-amz-json-1.1"
            }
        val model =
            """
                ${'$'}version: "2"
                namespace test.documentnull
                use aws.protocols#$protocol
                @$protocol
                service Documents { version: "1", operations: [Inspect] }
                @http(method: "POST", uri: "/document")
                operation Inspect {
                    input := { value: Document }
                    output := {}
                }
                """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                cargoCommand = "cargo test --quiet --lib -- --nocapture",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { context, crate ->
            crate.testModule {
                tokioTest("handler_document_null_is_not_absence") {
                    rustTemplate(
                        """
                            use std::sync::{Arc, Mutex};
                            let inputs = Arc::new(Mutex::new(#{Vec}::new()));
                            let received = inputs.clone();
                            let config = crate::service::DocumentsConfig::builder().build();
                            let service = crate::service::Documents::builder(config)
                                .inspect(move |input: crate::input::InspectInput| {
                                    received.lock().unwrap().push(input.value);
                                    async { crate::output::InspectOutput::builder().build() }
                                })
                                .build_unchecked();
                            for (body, expected) in [
                                (b"{\"value\":null}".as_slice(), #{Some}(#{Document}::Null)),
                                (b"{}".as_slice(), #{None}),
                                (b"{\"value\":true}".as_slice(), #{Some}(#{Document}::Bool(true))),
                                (b"{\"value\":[null,false]}".as_slice(), #{Some}(#{Document}::Array(vec![#{Document}::Null, #{Document}::Bool(false)]))),
                            ] {
                                let request = #{Http}::Request::builder().method("POST")
                                    .uri("${if (protocol == "restJson1") {
                            "/document"
                        } else {
                            "/"
                        }}")
                                    .header("content-type", "$contentType")
                                    .header("x-amz-target", "Documents.Inspect")
                                    .body(#{Server}::body::to_boxed_sync(body)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let headers = response.headers().clone();
                                let response_body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                                let actual = inputs.lock().unwrap().pop().expect("handler ran");
                                println!("$protocol schemaSerde=$schemaSerde {body:?}: {status} {headers:?} {response_body:?}; input {actual:?}");
                                assert_eq!(status, 200);
                                assert_eq!(response_body.as_ref(), b"{}");
                                assert_eq!(actual, expected, "{body:?}");
                            }
                            """,
                        *preludeScope,
                        "Document" to RuntimeType.document(context.runtimeConfig),
                        "Http" to RuntimeType.http(context.runtimeConfig),
                        "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                        "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        "Tower" to ServerCargoDependency.tower(context).toType(),
                    )
                }
            }
        }
    }
}

/**
 * Sends absent, null, repeated, and valid required members to generated legacy and schema
 * services. It checks success versus validation rejection and compares the resulting protocol
 * responses. This catches changes in required-member assignment when nulls or duplicate members
 * are encountered during request deserialization.
 */
@Tag("server-compatibility")
internal class RequiredMemberNullCompatibilityTest {
    // Generated serde REST JSON writes fieldList before message; schema writes them in model order.
    // Preserve the raw captures, but compare validation errors as JSON objects.
    // Status and every header stay exact, as do successful and RPC response bytes.
    private fun responses(
        path: Path,
        protocol: String,
    ) = Files.readAllLines(path).map { record ->
        if (protocol == "restJson1" && record.contains(": 400 ") && record.contains("\"x-amzn-errortype\": \"ValidationException\"")) {
            val bodyStart = record.lastIndexOf(" [")
            val body =
                record.substring(bodyStart + 1).removeSurrounding("[", "]")
                    .split(", ").map { it.toInt().toByte() }.toByteArray()
            record.substring(0, bodyStart) to Node.parse(body.toString(Charsets.UTF_8))
        } else {
            record to null
        }
    }

    @Test fun `required null absent and duplicate values match legacy responses`() {
        val run = java.nio.file.Files.createTempDirectory("required-null-parity-")
        for (protocol in listOf("restJson1", "awsJson1_0", "awsJson1_1")) {
            for (schemaSerde in listOf(false, true)) {
                val observation = run.resolve("$protocol-$schemaSerde.txt")
                val model =
                    """
                    ${'$'}version: "2"
                    namespace test.review
                    use aws.protocols#$protocol
                    @$protocol
                    service Review { version: "1", operations: [Read] }
                    @http(method: "POST", uri: "/read")
                    operation Read { input := { @required text: String }, output := {} }
                    """.asSmithyModel()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                        cargoCommand = "cargo test --quiet --lib -- --nocapture",
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    crate.testModule {
                        tokioTest("required_null_baseline") {
                            rustTemplate(
                                """
                                let app = crate::Review::builder(crate::ReviewConfig::builder().build())
                                    .read(|_: crate::input::ReadInput| async { Ok::<_, crate::error::ReadError>(crate::output::ReadOutput::builder().build()) }).build().unwrap();
                                let mut records = #{String}::new();
                                for input in ["{}", "{\"text\":null}", "{\"text\":\"ok\"}", "{\"text\":\"ok\",\"text\":null}"] {
                                    let request = #{Http}::Request::post(${if (protocol == "restJson1") {
                                    "\"/read\""
                                } else {
                                    "\"/\""
                                }})
                                        .header("content-type", ${if (protocol == "restJson1") {
                                    "\"application/json\""
                                } else {
                                    "\"application/x-amz-json-${if (protocol == "awsJson1_0") {
                                        "1.0"
                                    } else {
                                        "1.1"
                                    }}\""
                                }})
                                        .header("x-amz-target", "Review.Read")
                                        .body(#{Server}::body::to_boxed_sync(input)).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                    let status = response.status().as_u16();
                                    let headers = response.headers().clone();
                                    #{Collect}
                                    records.push_str(&format!("{input:?}: {status} {headers:?} {:?}\n", body.as_ref()));
                                    assert_eq!(status, if input.contains("ok") { 200 } else { 400 });
                                    if !input.contains("ok") { assert!(#{String}::from_utf8_lossy(&body).contains("ValidationException") || headers.get("x-amzn-errortype").is_some()); }
                                }
                                ::std::fs::write(${observation.toString().dq()}, records).unwrap();
                                """,
                                *RuntimeType.preludeScope,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                            )
                        }
                    }
                }
            }
            assertEquals(responses(run.resolve("$protocol-false.txt"), protocol), responses(run.resolve("$protocol-true.txt"), protocol), "$protocol: $run")
        }
    }
}
