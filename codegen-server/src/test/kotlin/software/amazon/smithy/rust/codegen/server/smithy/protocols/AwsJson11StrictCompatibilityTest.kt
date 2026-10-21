/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertAll
import org.junit.jupiter.api.Assertions.assertEquals
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

/**
 * Compare complete HTTP responses from independently generated AWS JSON 1.1 services.
 * HTTP binding traits deliberately remain in the model, including custom success/error codes.
 */
internal class AwsJson11StrictCompatibilityTest {
    @Test
    fun `generated AWS JSON 11 servers agree for populated payloads and HTTP binding traits`() {
        val root = Path.of(System.getenv("SCHEMA_COMPAT_ROOT") ?: "..").toAbsolutePath().normalize()
        val modelText = Files.readString(root.resolve("tools/schema-serde-compat/models/response-strict-aws-json-11.smithy"))
        val model = modelText.asSmithyModel()
        val observations =
            Files.createDirectories(
                Path.of(System.getenv("SCHEMA_COMPAT_RUN") ?: root.resolve("tools/schema-serde-compat/.runs/response-${java.util.UUID.randomUUID()}").toString())
                    .resolve("aws-json-11-strict"),
            )
        System.err.println("AWS JSON 1.1 strict comparison observations: $observations")
        Files.writeString(observations.resolve("model.smithy"), modelText)
        val configurations = listOf("legacy" to false, "schema" to true)
        for ((configuration, schemaSerde) in configurations) {
            val snapshot = observations.resolve("$configuration.tsv")
            val generated =
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings =
                            ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode().let { settings ->
                                System.getenv("SCHEMA_COMPAT_RUNTIME")?.let {
                                    settings.toBuilder().withMember("runtimeConfig", ObjectNode.parse("""{"relativePath":"$it"}""")).build()
                                } ?: settings
                            },
                        cargoCommand = "cargo test --quiet --lib --tests",
                        overrideTestDir = observations.resolve("generated-${if (schemaSerde) "schema" else "legacy"}").toFile(),
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("record_complete_http_responses") {
                            rustTemplate(
                                """
                                fn target(state: &str) -> #{Option}<crate::model::Target> {
                                    match state {
                                        "absent" => #{None},
                                        "empty" => #{Some}(crate::model::Target::builder().build()),
                                        "empty-collections" => #{Some}(crate::model::Target::builder()
                                            .items(#{Some}(vec![])).entries(#{Some}(#{Default}::default())).build()),
                                        _ => #{Some}(crate::model::Target::builder()
                                            .child(#{Some}("value \"quote\"\n".into()))
                                            .nested(#{Some}(crate::model::Nested::builder()
                                                .value(#{Some}("nested".into())).flag(#{Some}(false))
                                                .number(#{Some}(0)).build()))
                                            .items(#{Some}(vec!["first".into(), "".into(), "second".into()]))
                                            .entries(#{Some}([("key".into(), "value".into())].into_iter().collect())).build()),
                                    }
                                }
                                fn choice(state: &str) -> #{Option}<crate::model::Choice> {
                                    match state {
                                        "absent" => #{None},
                                        "empty" => #{Some}(crate::model::Choice::Text("".into())),
                                        "nested" => #{Some}(crate::model::Choice::Nested(target("full").unwrap())),
                                        "unit" => #{Some}(crate::model::Choice::Unit),
                                        _ => #{Some}(crate::model::Choice::Text("value".into())),
                                    }
                                }
                                let config = crate::service::PayloadsConfig::builder().build();
                                let service = crate::service::Payloads::builder(config)
                                    .struct_payload(|input: crate::input::StructPayloadInput| async move {
                                        #{Ok}::<_, crate::error::StructPayloadError>(
                                            crate::output::StructPayloadOutput::builder().body(target(&input.state))
                                                .header(#{Some}("output".into())).code(#{Some}(202)).build())
                                    })
                                    .union_payload(|input: crate::input::UnionPayloadInput| async move {
                                        #{Ok}::<_, crate::error::UnionPayloadError>(
                                            crate::output::UnionPayloadOutput::builder().body(choice(&input.state)).build())
                                    })
                                    .document(|input: crate::input::DocumentInput| async move {
                                        #{Ok}::<_, crate::error::DocumentError>(crate::output::DocumentOutput::builder().header(#{Some}("document".into()))
                                            .code(if input.state == "absent" { #{None} } else { #{Some}(202) })
                                            .body(target(&input.state)).choice(choice(&input.state)).build())
                                    })
                                    .fail(|input: crate::input::FailInput| async move {
                                        #{Err}::<crate::output::FailOutput, _>(crate::error::FailError::Failure(
                                            crate::error::Failure::builder()
                                                .header(#{Some}("error".into())).body(target(&input.state)).build()
                                        ))
                                    })
                                    .build_unchecked();
                                let mut records = #{String}::from("case\tstatus\theaders\tbodyHex\n");
                                for operation in ["StructPayload", "UnionPayload", "Document", "Fail"] {
                                    for state in ["absent", "empty", "empty-collections", "full", "nested", "unit"] {
                                        let case = format!("{operation}/{state}");
                                        let request = #{Http}::Request::builder().method("POST").uri("/")
                                            .header("content-type", "application/x-amz-json-1.1")
                                            .header("x-amz-target", format!("Payloads.{operation}"))
                                            .body(#{Server}::body::to_boxed_sync(format!(r##"{{"state":"{state}"}}"##))).unwrap();
                                        let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                        let status = response.status().as_u16();
                                        let mut headers = response.headers().iter()
                                            .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
                                            .collect::<#{Vec}<_>>();
                                        headers.sort();
                                        let body = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 1024 * 1024))
                                            .await.unwrap().to_bytes();
                                        if let #{Some}((_, value)) = headers.iter().find(|(name, _)| name == "content-length") {
                                            assert_eq!(#{String}::from_utf8(value.clone()).unwrap().parse::<usize>().unwrap(),
                                                body.len(), "{case}");
                                        }
                                        let hex = body.iter().map(|byte| format!("{byte:02x}")).collect::<#{String}>();
                                        records.push_str(&format!("{case}\t{status}\t{headers:?}\t{hex}\n"));
                                    }
                                }
                                ::std::fs::write("${snapshot.toAbsolutePath()}", records).unwrap();
                                """,
                                *RuntimeType.preludeScope,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                            )
                        }
                    }
                }
            Files.writeString(
                observations.resolve("$configuration-generated.txt"),
                generated.joinToString("\n") {
                    it.path.toString()
                },
            )
        }
        val legacy = Files.readString(observations.resolve("legacy.tsv"))
        assertAll(
            "Compare schemaSerde with the observed mainline AWS JSON response bindings",
            {
                for (row in legacy.lineSequence().drop(1).filter { it.isNotBlank() }) {
                    val columns = row.split('\t')
                    // Fresh mainline at 9267f0e25 honors this model's HTTP status traits.
                    val expected = if (columns[0].startsWith("Fail/")) 409 else 201
                    assertEquals(expected, columns[1].toInt(), "Legacy fixture status: ${columns[0]}")
                }
            },
            {
                assertEquals(legacy, Files.readString(observations.resolve("schema.tsv")))
            },
        )
    }
}
