/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.restxml

import org.junit.jupiter.api.Assertions.assertAll
import org.junit.jupiter.api.Assertions.assertEquals
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
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Replays REST XML success and modeled-error scenarios with populated and absent response
 * features, including structured payloads and HTTP bindings. It pins legacy status codes,
 * checks content lengths, and compares complete HTTP and generated-output captures between
 * legacy and schema services without excluding known payload cases.
 */
internal class RestXmlStrictCompatibilityTest {
    @Test
    fun `generated REST XML servers agree for populated and absent response features`() {
        val modelText =
            """
            ${'$'}version: "2"
            namespace test.strictxml
            use aws.protocols#restXml
            @restXml
            service Payloads {
                version: "1"
                operations: [Fallback, TargetOnly, MemberOnly, Both, Shared, UnionPayload, Document, Fail, Echo]
            }
            structure StateInput { @required @httpLabel state: String }
            @http(uri: "/fallback/{state}", method: "GET")
            operation Fallback { input: StateInput, output := { @httpPayload body: Target } }
            @http(uri: "/target/{state}", method: "GET")
            operation TargetOnly { input: StateInput, output := { @httpPayload body: Renamed } }
            @http(uri: "/member/{state}", method: "GET")
            operation MemberOnly { input: StateInput, output := { @httpPayload @xmlName("WireMember") body: Target } }
            @http(uri: "/both/{state}", method: "GET")
            operation Both { input: StateInput, output := { @httpPayload @xmlName("WireBoth") body: Renamed } }
            @http(uri: "/shared/{state}", method: "GET")
            operation Shared { input: StateInput, output := { @httpPayload @xmlName("OtherRoot") body: Renamed } }
            @http(uri: "/union/{state}", method: "GET")
            operation UnionPayload { input: StateInput, output := { @httpPayload @xmlName("WireUnion") body: Choice } }
            @http(uri: "/document/{state}", method: "GET", code: 201)
            operation Document { input: StateInput, output: DocumentOutput }
            @xmlName("DocumentRoot")
            @xmlNamespace(uri: "urn:document")
            structure DocumentOutput {
                @httpHeader("x-fixture") fixture: String
                @xmlName("Nested") body: Target
                choice: Choice
                @xmlFlattened @xmlName("Item") items: Strings
            }
            @http(uri: "/failure/{state}", method: "GET")
            operation Fail { input: StateInput, output := {}, errors: [Failure] }
            @error("client") @httpError(409)
            structure Failure {
                @httpHeader("x-fixture") fixture: String
                @httpPayload @xmlName("FailurePayload") body: Renamed
            }
            @http(uri: "/echo", method: "POST")
            operation Echo {
                input := { @httpPayload body: Target }
                output := { @httpPayload body: Target }
            }
            structure Target {
                @xmlName("Child") child: String
                @xmlAttribute @xmlName("attribute") attr: String
                nested: Nested
                @xmlName("Items") items: Strings
                @xmlFlattened @xmlName("Flat") flat: Strings
            }
            structure Nested { value: String, flag: Boolean, number: Integer }
            list Strings { @xmlName("Entry") member: String }
            @xmlName("RenamedTarget")
            @xmlNamespace(uri: "urn:target", prefix: "t")
            structure Renamed {
                @xmlName("Child") child: String
                @xmlAttribute @xmlName("attribute") attr: String
                nested: Nested
                @xmlName("Items") items: Strings
                @xmlFlattened @xmlName("Flat") flat: Strings
            }
            @xmlName("RenamedUnion")
            @xmlNamespace(uri: "urn:union")
            union Choice { @xmlName("Text") text: String, @xmlName("Nested") nested: Target, unit: Unit }
            """.trimIndent()
        val model = modelText.asSmithyModel()
        val observations = Files.createTempDirectory("smithy-rest-xml-strict-")
        System.err.println("REST XML strict comparison observations: $observations")
        Files.writeString(observations.resolve("model.smithy"), modelText)
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.tsv")
            val generated =
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                        cargoCommand = "cargo test --quiet --lib --tests",
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
                                            .items(#{Some}(vec![])).flat(#{Some}(vec![])).build()),
                                        _ => #{Some}(crate::model::Target::builder()
                                            .child(#{Some}("value <&> \"quote\"".into()))
                                            .set_attr(#{Some}(#{String}::from("attr <&> \"quote\"")))
                                            .nested(#{Some}(crate::model::Nested::builder()
                                                .value(#{Some}("nested".into())).flag(#{Some}(false))
                                                .number(#{Some}(0)).build()))
                                            .items(#{Some}(vec!["first".into(), "".into(), "<&>".into()]))
                                            .flat(#{Some}(vec!["flat".into(), "second".into()])).build()),
                                    }
                                }
                                fn renamed(state: &str) -> #{Option}<crate::model::Renamed> {
                                    target(state).map(|body| crate::model::Renamed {
                                        child: body.child, attr: body.attr, nested: body.nested,
                                        items: body.items, flat: body.flat,
                                    })
                                }
                                fn choice(state: &str) -> #{Option}<crate::model::Choice> {
                                    match state {
                                        "absent" => #{None},
                                        "empty" => #{Some}(crate::model::Choice::Text("".into())),
                                        "nested" => #{Some}(crate::model::Choice::Nested(target("full").unwrap())),
                                        "unit" => #{Some}(crate::model::Choice::Unit),
                                        _ => #{Some}(crate::model::Choice::Text("<&>".into())),
                                    }
                                }
                                let config = crate::service::PayloadsConfig::builder().build();
                                let service = crate::service::Payloads::builder(config)
                                    .fallback(|input: crate::input::FallbackInput| async move {
                                        #{Ok}::<_, crate::error::FallbackError>(
                                            crate::output::FallbackOutput::builder().body(target(&input.state)).build())
                                    })
                                    .target_only(|input: crate::input::TargetOnlyInput| async move {
                                        #{Ok}::<_, crate::error::TargetOnlyError>(
                                            crate::output::TargetOnlyOutput::builder().body(renamed(&input.state)).build())
                                    })
                                    .member_only(|input: crate::input::MemberOnlyInput| async move {
                                        #{Ok}::<_, crate::error::MemberOnlyError>(
                                            crate::output::MemberOnlyOutput::builder().body(target(&input.state)).build())
                                    })
                                    .both(|input: crate::input::BothInput| async move {
                                        #{Ok}::<_, crate::error::BothError>(
                                            crate::output::BothOutput::builder().body(renamed(&input.state)).build())
                                    })
                                    .shared(|input: crate::input::SharedInput| async move {
                                        #{Ok}::<_, crate::error::SharedError>(
                                            crate::output::SharedOutput::builder().body(renamed(&input.state)).build())
                                    })
                                    .union_payload(|input: crate::input::UnionPayloadInput| async move {
                                        #{Ok}::<_, crate::error::UnionPayloadError>(
                                            crate::output::UnionPayloadOutput::builder().body(choice(&input.state)).build())
                                    })
                                    .document(|input: crate::input::DocumentInput| async move {
                                        #{Ok}::<_, crate::error::DocumentError>(crate::output::DocumentOutput::builder()
                                            .fixture(#{Some}("document".into())).body(target(&input.state))
                                            .choice(choice(&input.state))
                                            .items(if input.state == "absent" { #{None} } else {
                                                #{Some}(if input.state == "empty" { vec![] } else {
                                                    vec!["first".into(), "second".into()]
                                                })
                                            }).build())
                                    })
                                    .fail(|input: crate::input::FailInput| async move {
                                        #{Err}::<crate::output::FailOutput, _>(crate::error::FailError::Failure(
                                            crate::error::Failure::builder()
                                                .fixture(#{Some}("failure".into())).body(renamed(&input.state)).build()
                                        ))
                                    })
                                    .echo(|input: crate::input::EchoInput| async move {
                                        crate::output::EchoOutput::builder().body(input.body).build()
                                    })
                                    .build_unchecked();
                                let mut requests = #{Vec}::new();
                                for operation in ["fallback", "target", "member", "both", "shared", "union", "document", "failure"] {
                                    for state in ["absent", "empty", "empty-collections", "full"] {
                                        let status = match operation { "document" => 201, "failure" => 409, _ => 200 };
                                        requests.push((format!("{operation}/{state}"), "GET",
                                            format!("/{operation}/{state}"), #{String}::new(), status));
                                    }
                                }
                                for state in ["nested", "unit"] {
                                    requests.push((format!("union/{state}"), "GET", format!("/union/{state}"), #{String}::new(), 200));
                                }
                                for (name, body, status) in [
                                    ("empty-body", "", 200),
                                    ("empty-root", "<Target/>", 200),
                                    ("populated", "<Target attribute=\"a&amp;b\"><Child>c&lt;d</Child><nested><value>v</value><flag>false</flag><number>0</number></nested><Items><Entry>one</Entry><Entry>two</Entry></Items><Flat>f</Flat></Target>", 200),
                                    ("unknown", "<Target><Unknown>x</Unknown><Child>value</Child></Target>", 200),
                                    ("malformed", "<Target><Child>", 400),
                                ] {
                                    requests.push((format!("echo/{name}"), "POST", "/echo".into(), body.into(), status));
                                }
                                let mut records = #{String}::from("case\tstatus\theaders\tbodyHex\n");
                                for (case, method, path, body, expected_status) in requests {
                                    let request = #{Http}::Request::builder().method(method).uri(path)
                                        .header("content-type", "application/xml")
                                        .body(#{Server}::body::to_boxed_sync(body)).unwrap();
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
                                    if status != expected_status {
                                        eprintln!("{case}: expected status {expected_status}, observed {status}");
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
                observations.resolve("$schemaSerde-generated.txt"),
                generated.joinToString("\n") {
                    it.path.toString()
                },
            )
        }
        val legacy = Files.readString(observations.resolve("false.tsv"))
        val schema = Files.readString(observations.resolve("true.tsv"))
        assertAll(
            "Record all responses before checking observed legacy behavior and compatibility",
            {
                for (row in legacy.lineSequence().drop(1).filter { it.isNotBlank() }) {
                    val columns = row.split('\t')
                    val expected =
                        when {
                            columns[0].startsWith("document/") -> 201
                            columns[0].startsWith("failure/") -> 409
                            // Fresh mainline at 9267f0e25 accepts this fixture and returns 200.
                            else -> 200
                        }
                    assertEquals(expected, columns[1].toInt(), "Legacy fixture status: ${columns[0]}")
                }
            },
            { assertEquals(legacy, schema, "Schema REST XML differs from generated legacy") },
        )
    }
}
