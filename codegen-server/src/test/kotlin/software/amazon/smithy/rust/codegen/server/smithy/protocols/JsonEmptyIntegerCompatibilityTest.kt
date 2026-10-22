/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

internal class JsonEmptyIntegerCompatibilityTest {
    @Test
    fun `negative fractions with no integer part preserve legacy JSON behavior`() {
        val observations = Files.createTempDirectory("json-empty-integer-")
        System.err.println("JSON number compatibility observations: $observations")
        for (protocol in listOf("restJson1", "awsJson1_0", "awsJson1_1")) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.jsonnumbers
                use aws.protocols#$protocol
                @$protocol service Numbers { version: "1", operations: [Echo] }
                @http(method: "POST", uri: "/echo")
                operation Echo { input := { value: Double }, output := { value: Double } }
                """.asSmithyModel()
            val contentType =
                when (protocol) {
                    "awsJson1_0" -> "application/x-amz-json-1.0"
                    "awsJson1_1" -> "application/x-amz-json-1.1"
                    else -> "application/json"
                }
            for (schemaSerde in listOf(false, true)) {
                val snapshot = observations.resolve("$protocol-$schemaSerde.txt")
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ObjectNode.parse("""{"codegen":{"http-1x":true,"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                        overrideTestDir = observations.resolve("$protocol-$schemaSerde").toFile(),
                        cargoCommand = "cargo test --quiet --lib -- --nocapture",
                    ),
                    testCoverage = HttpTestType.Default,
                ) { context, crate ->
                    check(context.usesSchemaHttpSerde == schemaSerde)
                    crate.testModule {
                        tokioTest("number_wire_observations") {
                            rustTemplate(
                                """
                                let app = crate::Numbers::builder(crate::NumbersConfig::builder().build())
                                    .echo(|input: crate::input::EchoInput| async move {
                                        crate::output::EchoOutput { value: input.value }
                                    }).build().unwrap();
                                let mut records = #{String}::new();
                                for (name, input) in [
                                    ("known", r##"{"value":-.123}"##),
                                    ("exponent", r##"{"value":-.123e2}"##),
                                    ("skipped", r##"{"unknown":-.123,"value":1.0}"##),
                                    ("standard", r##"{"value":-0.123}"##),
                                    ("positive", r##"{"value":.123}"##),
                                    ("no-digits", r##"{"value":-.}"##),
                                    ("no-fraction", r##"{"value":-.e2}"##),
                                ] {
                                    let request = #{Http}::Request::post(${(if (protocol == "restJson1") "/echo" else "/").dq()})
                                        .header("content-type", ${contentType.dq()})
                                        .header("x-amz-target", "Numbers.Echo")
                                        .body(#{Server}::body::to_boxed_sync(input)).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                    let status = response.status().as_u16();
                                    // Established by running schemaSerde=false before changing the codec.
                                    assert_eq!(status, if matches!(name, "positive" | "no-digits" | "no-fraction") { 400 } else { 200 }, "{name}");
                                    let mut headers: #{Vec}<_> = response.headers().iter()
                                        .map(|(key, value)| (key.to_string(), value.as_bytes().to_vec())).collect();
                                    headers.sort();
                                    #{Collect}
                                    let record = format!("{name}: {status} {headers:?} {:?}\n", body.as_ref());
                                    print!("{record}");
                                    records.push_str(&record);
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
            assertEquals(
                Files.readString(observations.resolve("$protocol-false.txt")),
                Files.readString(observations.resolve("$protocol-true.txt")),
                "$protocol: $observations",
            )
        }
    }
}
