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
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path

internal class TrailingSlashLabelsCompatibilityTest {
    @Test
    fun `trailing template slashes preserve legacy routing and label values`() {
        val root = Path.of(System.getenv("SCHEMA_COMPAT_ROOT") ?: "..").toAbsolutePath().normalize()
        val model = Files.readString(root.resolve("tools/schema-serde-compat/models/trailing-slash-labels.smithy")).asSmithyModel()
        val run = System.getenv("SCHEMA_COMPAT_RUN")?.let(Path::of) ?: Files.createTempDirectory("trailing-slash-labels-")
        val protocols = listOf("JsonLabels", "XmlLabels")
        // Generate and exercise both generated serde services before testing schema serde.
        for (schemaSerde in listOf(false, true)) {
            for (service in protocols) {
                val mode = if (schemaSerde) "schema" else "legacy"
                val settings =
                    Node.objectNodeBuilder().withMember(
                        "codegen",
                        Node.objectNodeBuilder().withMember("http-1x", true).withMember("schemaSerde", schemaSerde).build(),
                    )
                System.getenv("SCHEMA_COMPAT_RUNTIME")?.let {
                    settings.withMember("runtimeConfig", Node.objectNodeBuilder().withMember("relativePath", it).build())
                }
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
