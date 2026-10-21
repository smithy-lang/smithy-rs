/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/** Pins the generated serde behavior behind the two executed upstream header expected failures. */
internal class RestXmlHeaderCompatibilityTest {
    @Test
    fun `empty headers and bound prefix collisions match legacy`() {
        val observations = Files.createTempDirectory("xml-header-parity-")
        val model =
            """
            ${'$'}version: "2"
            namespace test.xmlheaders
            use aws.protocols#restXml
            @restXml service Headers { version: "1", operations: [Empty, Prefix] }
            @http(method: "GET", uri: "/empty")
            operation Empty { input := {}, output := {
                @httpHeader("X-A") a: String
                @httpHeader("X-B") b: String
                @httpHeader("X-C") c: Strings
            } }
            @http(method: "GET", uri: "/prefix")
            operation Prefix { input := {}, output := {
                @httpPrefixHeaders("") prefixHeaders: StringMap
                @httpHeader("hello") specificHeader: String
            } }
            list Strings { member: String }
            map StringMap { key: String, value: String }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("legacy_header_responses") {
                        rustTemplate(
                            """
                            let app = crate::Headers::builder(crate::HeadersConfig::builder().build())
                                .empty(|_: crate::input::EmptyInput| async {
                                    crate::output::EmptyOutput::builder().b(#{Some}("".into())).c(#{Some}(vec![])).build()
                                })
                                .prefix(|_: crate::input::PrefixInput| async {
                                    crate::output::PrefixOutput::builder()
                                        .specific_header(#{Some}("There".into()))
                                        .prefix_headers(#{Some}(::std::collections::HashMap::from([
                                            ("x-foo".into(), "Foo".into()), ("hello".into(), "Hello".into())
                                        ]))).build()
                                }).build().unwrap();
                            let mut records = #{String}::new();
                            for path in ["/empty", "/prefix"] {
                                let request = #{Http}::Request::get(path)
                                    .body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                assert_eq!(status, 200);
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                records.push_str(&format!("{path}: {status} {headers:?} {:?}\n", body.as_ref()));
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
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
        val legacy = Files.readString(observations.resolve("false.txt"))
        // Observed against generated fresh-mainline servers with this same model and fixtures.
        assertEquals(
            "/empty: 200 [(\"content-length\", \"0\")] []\n" +
                "/prefix: 200 [(\"content-length\", \"0\"), (\"hello\", \"Hello\"), (\"hello\", \"There\"), (\"x-foo\", \"Foo\")] []\n",
            legacy,
            observations.toString(),
        )
        assertEquals(legacy, Files.readString(observations.resolve("true.txt")), observations.toString())
    }
}
