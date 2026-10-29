/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.routing

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Tag
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
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
 * Sends REST JSON and REST XML requests to plain and greedy label routes whose modeled URI
 * ends in a slash. It checks which paths match, the exact label received by the handler, and
 * success or unknown-operation response framing. Identical cases run with legacy and schema
 * serde to preserve the legacy treatment of trailing template slashes.
 */
@Tag("server-compatibility")
internal class TrailingSlashLabelCompatibilityTest {
    @Test
    fun `legacy labels ignore trailing template slashes`() = checkLabels(schemaSerde = false)

    @Test
    fun `schema labels match legacy trailing template slashes`() = checkLabels(schemaSerde = true)

    private fun checkLabels(schemaSerde: Boolean) {
        for (protocol in listOf("restJson1", "restXml")) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.trailinglabels
                use aws.protocols#$protocol
                @$protocol
                service Labels { version: "1", operations: [Pet, Data, File] }
                @http(method: "GET", uri: "/pets/{name}/")
                operation Pet {
                    input := { @required @httpLabel name: String }
                    output: LabelOutput
                }
                @http(method: "GET", uri: "/data/{key+}/")
                operation Data {
                    input := { @required @httpLabel key: String }
                    output: LabelOutput
                }
                @http(method: "GET", uri: "/files/{key+}/meta/")
                operation File {
                    input := { @required @httpLabel key: String }
                    output: LabelOutput
                }
                structure LabelOutput { @httpPayload value: String }
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
                    tokioTest("trailing_template_slashes_match_legacy") {
                        rustTemplate(
                            """
                            let config = crate::service::LabelsConfig::builder().build();
                            let service = crate::service::Labels::builder(config)
                                .pet(|input: crate::input::PetInput| async move {
                                    #{Ok}::<_, crate::error::PetError>(
                                        crate::output::PetOutput::builder().value(#{Some}(input.name)).build())
                                })
                                .data(|input: crate::input::DataInput| async move {
                                    #{Ok}::<_, crate::error::DataError>(
                                        crate::output::DataOutput::builder().value(#{Some}(input.key)).build())
                                })
                                .file(|input: crate::input::FileInput| async move {
                                    #{Ok}::<_, crate::error::FileError>(
                                        crate::output::FileOutput::builder().value(#{Some}(input.key)).build())
                                })
                                .build_unchecked();
                            for (path, status, label) in [
                                ("/pets/rex", 200, #{Some}("rex")),
                                ("/pets/rex%20jr", 200, #{Some}("rex jr")),
                                ("/pets/rex/", 404, #{None}),
                                ("/data/a/b", 200, #{Some}("a/b")),
                                ("/data/a/b/", 200, #{Some}("a/b/")),
                                ("/data//a//b/", 200, #{Some}("/a//b/")),
                                ("/data/", 200, #{Some}("")),
                                ("/data", 404, #{None}),
                                ("/files/a/b/meta", 200, #{Some}("a/b")),
                                ("/files//meta", 200, #{Some}("")),
                                ("/files/a/b/meta/", 404, #{None}),
                            ] {
                                let request = #{Http}::Request::builder().method("GET").uri(path)
                                    .body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                let actual_status = response.status().as_u16();
                                let headers = response.headers().clone();
                                let body = #{HttpBodyUtil}::BodyExt::collect(response.into_body())
                                    .await.unwrap().to_bytes();
                                println!("$protocol schemaSerde=$schemaSerde {path}: {actual_status} {headers:?} {body:?}");
                                assert_eq!(actual_status, status, "{path}: {body:?}");
                                if let #{Some}(label) = label {
                                    assert_eq!(headers["content-type"], "text/plain", "{path}");
                                    assert_eq!(headers["content-length"], label.len().to_string(), "{path}");
                                    assert_eq!(body.as_ref(), label.as_bytes(), "{path}");
                                } else if "$protocol" == "restJson1" {
                                    assert_eq!(headers["content-type"], "application/json", "{path}");
                                    assert_eq!(headers["x-amzn-errortype"], "UnknownOperationException", "{path}");
                                    assert_eq!(body.as_ref(), b"{}", "{path}");
                                } else {
                                    assert_eq!(headers["content-type"], "application/xml", "{path}");
                                    assert!(body.is_empty(), "{path}: {body:?}");
                                }
                            }
                            """,
                            *preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "HttpBodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                        )
                    }
                }
            }
        }
    }
}

/**
 * Uses the dedicated label fixture to replay plain, trailing-slash, greedy, suffix, and query
 * routes through legacy and schema REST JSON/XML servers. It pins legacy routing statuses and
 * handler-visible label strings, then compares complete response captures. This covers label
 * extraction and routing together rather than only parsing a URI template.
 */
@Tag("server-compatibility")
internal class TrailingSlashLabelsCompatibilityTest {
    @Test
    fun `trailing template slashes preserve legacy routing and label values`() {
        val root = Path.of("..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("codegen-server/src/test/resources/protocol-compat/models/trailing-slash-labels.smithy")).asSmithyModel()
        val run = Files.createTempDirectory("trailing-slash-labels-")
        val protocols = listOf("JsonLabels", "XmlLabels")
        // Generate and exercise both generated serde services before testing schema serde.
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
                        service = "compat.labels#$service",
                        additionalSettings = settings.build(),
                        overrideTestDir = run.resolve("$service-$mode").toFile(),
                        command = { path ->
                            val log = run.resolve("$service-$mode-cargo.log")
                            val process =
                                ProcessBuilder("cargo", "test", "--quiet", "--lib", "trailing_slash_labels")
                                    .directory(path.toFile()).redirectErrorStream(true).redirectOutput(log.toFile())
                            process.environment()["RUSTFLAGS"] = "-D warnings"
                            check(process.start().waitFor() == 0) { "Cargo failed: $log" }
                        },
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("trailing_slash_labels") {
                            rustTemplate(
                                """
                                use #{Tower}::ServiceExt;
                                use #{BodyUtil}::BodyExt;
                                let app = crate::$service::builder(crate::${service}Config::builder().build())
                                    .plain(|input: crate::input::PlainInput| async move { Ok::<_, crate::error::PlainError>(crate::output::PlainOutput { value: Some(input.value) }) })
                                    .trailing(|input: crate::input::TrailingInput| async move { Ok::<_, crate::error::TrailingError>(crate::output::TrailingOutput { value: Some(input.value) }) })
                                    .greedy(|input: crate::input::GreedyInput| async move { Ok::<_, crate::error::GreedyError>(crate::output::GreedyOutput { value: Some(input.value) }) })
                                    .greedy_trailing(|input: crate::input::GreedyTrailingInput| async move { Ok::<_, crate::error::GreedyTrailingError>(crate::output::GreedyTrailingOutput { value: Some(input.value) }) })
                                    .suffix(|input: crate::input::SuffixInput| async move { Ok::<_, crate::error::SuffixError>(crate::output::SuffixOutput { value: Some(input.value) }) })
                                    .query(|input: crate::input::QueryInput| async move { Ok::<_, crate::error::QueryError>(crate::output::QueryOutput { value: Some(input.value) }) })
                                    .build().unwrap();
                                let cases = [
                                    ("/plain/rex", 200, Some("rex")),
                                    ("/plain/rex/", 404, None),
                                    ("/trailing/rex", 200, Some("rex")),
                                    ("/trailing/rex/", 404, None),
                                    ("/trailing/", 200, Some("")),
                                    ("/trailing", 404, None),
                                    ("/trailing/a%2Fb", 200, Some("a/b")),
                                    ("/trailing/a%20b", 200, Some("a b")),
                                    ("/trailing/a+b", 200, Some("a+b")),
                                    ("/trailing/a/b", 404, None),
                                    ("/trailing/rex?extra=1", 200, Some("rex")),
                                    ("/greedy/a/b", 200, Some("a/b")),
                                    ("/greedy/a/b/", 200, Some("a/b/")),
                                    ("/greedy/a//b", 200, Some("a//b")),
                                    ("/greedy-trailing/a/b", 200, Some("a/b")),
                                    ("/greedy-trailing/a/b/", 200, Some("a/b/")),
                                    ("/greedy-trailing/a/b//", 200, Some("a/b//")),
                                    ("/greedy-trailing/a//b", 200, Some("a//b")),
                                    ("/greedy-trailing//a/", 200, Some("/a/")),
                                    ("/greedy-trailing/", 200, Some("")),
                                    ("/greedy-trailing", 404, None),
                                    ("/greedy-trailing/a%2Fb", 200, Some("a/b")),
                                    ("/greedy-trailing/a/b?extra=1", 200, Some("a/b")),
                                    ("/suffix/a/b/end", 200, Some("a/b")),
                                    ("/suffix/a//end", 200, Some("a/")),
                                    ("/suffix//end", 200, Some("")),
                                    ("/suffix/a/b/end/", 404, None),
                                    ("/suffix/a/b/other", 404, None),
                                    ("/query/rex?kind=fixed", 200, Some("rex")),
                                    ("/query/rex/?kind=fixed", 404, None),
                                    ("/query/rex?kind=wrong", 404, None),
                                    ("/query/rex", 404, None),
                                ];
                                let mut observations = Vec::new();
                                for (uri, expected_status, expected_value) in cases {
                                    let request = #{Http}::Request::get(uri).body(#{Server}::body::Body::empty()).unwrap();
                                    let response = app.clone().oneshot(request).await.unwrap();
                                    let status = response.status().as_u16();
                                    let mut headers = response.headers().iter().map(|(name, value)|
                                        (name.as_str().to_owned(), value.as_bytes().to_vec())
                                    ).collect::<Vec<_>>();
                                    headers.sort();
                                    let body = response.into_body().collect().await.unwrap().to_bytes();
                                    // Check the live reference, so equal early rejections cannot pass.
                                    if ${!schemaSerde} {
                                        assert_eq!(status, expected_status, "legacy control: {uri}");
                                        if let Some(value) = expected_value {
                                            assert_eq!(body.as_ref(), value.as_bytes(), "legacy label: {uri}");
                                        }
                                    }
                                    observations.push(#{Json}::json!({"uri":uri, "status":status, "headers":headers, "body":body.to_vec()}));
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
