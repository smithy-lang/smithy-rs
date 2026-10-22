/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/**
 * Executes identical named cases through independently generated servers.
 * Includes REST payload errors even when they expose an existing regression.
 */
internal class RestJsonStrictCompatibilityTest {
    @Test
    fun `populated outputs and errors preserve the legacy HTTP contract`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.strictrestjson
            use aws.protocols#restJson1
            @restJson1
            service StrictRestJson {
                version: "1"
                operations: [DocumentResponse, StructResponse, UnionResponse, TextResponse, BlobResponse]
            }
            structure ModeInput { @httpQuery("case") mode: String }
            @http(method: "GET", uri: "/document", code: 201)
            operation DocumentResponse { input: ModeInput, output: DocumentOutput, errors: [Failure] }
            structure DocumentOutput {
                @jsonName("wireRecord") record: Record
                choice: Choice
                @httpResponseCode code: Integer
                @httpHeader("x-value") header: String
                @httpHeader("x-tags") tags: Strings
                @httpPrefixHeaders("x-meta-") metadata: StringMap
                @httpHeader("content-type") media: String
            }
            @http(method: "GET", uri: "/struct", code: 202)
            operation StructResponse { input: ModeInput, output := {
                @httpPayload body: Record
                @httpHeader("x-value") header: String
            }, errors: [PayloadFailure] }
            @http(method: "GET", uri: "/union", code: 200)
            operation UnionResponse { input: ModeInput, output := { @httpPayload body: Choice }, errors: [UnionFailure] }
            @http(method: "GET", uri: "/text", code: 200)
            operation TextResponse { input: ModeInput, output := { @httpPayload body: String } }
            @http(method: "GET", uri: "/blob", code: 200)
            operation BlobResponse { input: ModeInput, output := { @httpPayload body: Blob } }
            structure Record {
                @jsonName("wireChild") child: String
                items: Strings
                entries: StringMap
                nested: Nested
            }
            structure Nested { enabled: Boolean, number: Integer }
            union Choice { text: String, record: Record, items: Strings }
            list Strings { member: String }
            map StringMap { key: String, value: String }
            @error("client") @httpError(409)
            structure Failure {
                message: String
                record: Record
                @httpHeader("x-error") header: String
                @httpPrefixHeaders("x-error-meta-") metadata: StringMap
            }
            @error("client") @httpError(422)
            structure UnionFailure { @httpPayload body: Choice }
            @error("server") @httpError(503)
            structure PayloadFailure {
                @httpPayload body: Record
                @httpHeader("x-error") header: String
            }
            """.asSmithyModel()
        val observations = Files.createTempDirectory("smithy-rest-json-strict-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                crate.testModule {
                    tokioTest("strict_http_cases") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            use #{BodyUtil}::BodyExt;
                            fn record(empty: bool) -> crate::model::Record {
                                if empty {
                                    crate::model::Record { child: #{None}, items: #{Some}(#{Vec}::new()),
                                        entries: #{Some}(::std::collections::HashMap::new()), nested: #{None} }
                                } else {
                                    crate::model::Record {
                                        child: #{Some}("quote\" slash\\ newline\n café".into()),
                                        items: #{Some}(vec!["one".into(), "".into(), "two".into()]),
                                        entries: #{Some}([("alpha".into(), "value".into()), ("empty".into(), "".into())].into()),
                                        nested: #{Some}(crate::model::Nested { enabled: #{Some}(false), number: #{Some}(0) }),
                                    }
                                }
                            }
                            let service = crate::StrictRestJson::builder(crate::StrictRestJsonConfig::builder().build())
                                .document_response(|input: crate::input::DocumentResponseInput| async move {
                                    let mode = input.mode.as_deref().unwrap_or("absent");
                                    if mode.starts_with("error") {
                                        return #{Err}(crate::error::DocumentResponseError::Failure(crate::error::Failure {
                                            message: if mode == "error-absent" { #{None} } else { #{Some}("failure detail".into()) },
                                            record: if mode == "error-absent" { #{None} } else { #{Some}(record(mode == "error-empty")) },
                                            header: #{Some}("error-header".into()),
                                            metadata: #{Some}([("kind".into(), "ordinary".into())].into()),
                                        }));
                                    }
                                    #{Ok}(crate::output::DocumentResponseOutput {
                                        record: if mode == "absent" { #{None} } else { #{Some}(record(mode == "empty")) },
                                        choice: if mode == "absent" { #{None} } else { #{Some}(crate::model::Choice::Record(record(false))) },
                                        code: if mode == "custom-status" { #{Some}(206) } else { #{None} },
                                        header: #{Some}("header".into()),
                                        tags: #{Some}(vec!["zulu".into(), "alpha".into()]),
                                        metadata: #{Some}([("first".into(), "one".into()), ("second".into(), "two".into())].into()),
                                        media: if mode == "custom-media" { #{Some}("application/custom".into()) } else { #{None} },
                                    })
                                })
                                .struct_response(|input: crate::input::StructResponseInput| async move {
                                    let mode = input.mode.as_deref().unwrap_or("absent");
                                    if mode.starts_with("error") {
                                        return #{Err}(crate::error::StructResponseError::PayloadFailure(crate::error::PayloadFailure {
                                            body: if mode == "error-absent" { #{None} } else { #{Some}(record(mode == "error-empty")) },
                                            header: #{Some}("payload-error".into()),
                                        }));
                                    }
                                    #{Ok}(crate::output::StructResponseOutput {
                                        body: if mode == "absent" { #{None} } else { #{Some}(record(mode == "empty")) },
                                        header: #{Some}("payload-header".into()),
                                    })
                                })
                                .union_response(|input: crate::input::UnionResponseInput| async move {
                                    let mode = input.mode.as_deref().unwrap_or("absent");
                                    let body = match mode.strip_prefix("error-").unwrap_or(mode) {
                                        "absent" => #{None},
                                        "text" => #{Some}(crate::model::Choice::Text("variant".into())),
                                        "text-empty" => #{Some}(crate::model::Choice::Text("".into())),
                                        "items" => #{Some}(crate::model::Choice::Items(vec!["a".into(), "b".into()])),
                                        "items-empty" => #{Some}(crate::model::Choice::Items(#{Vec}::new())),
                                        "record-empty" => #{Some}(crate::model::Choice::Record(record(true))),
                                        _ => #{Some}(crate::model::Choice::Record(record(false))),
                                    };
                                    if mode.starts_with("error-") {
                                        return #{Err}(crate::error::UnionResponseError::UnionFailure(crate::error::UnionFailure { body }));
                                    }
                                    #{Ok}(crate::output::UnionResponseOutput { body })
                                })
                                .text_response(|input: crate::input::TextResponseInput| async move {
                                    crate::output::TextResponseOutput { body: match input.mode.as_deref().unwrap_or("absent") {
                                        "absent" => #{None}, "empty" => #{Some}("".into()),
                                        _ => #{Some}("raw \"text\"\n café".into()),
                                    } }
                                })
                                .blob_response(|input: crate::input::BlobResponseInput| async move {
                                    crate::output::BlobResponseOutput { body: match input.mode.as_deref().unwrap_or("absent") {
                                        "absent" => #{None}, "empty" => #{Some}(#{Types}::Blob::new([])),
                                        _ => #{Some}(#{Types}::Blob::new([0, 1, 255, 10])),
                                    } }
                                })
                                .build().unwrap();
                            let cases: &[(&str, &[&str])] = &[
                                ("document", &["absent", "empty", "populated", "custom-status", "custom-media", "error-absent", "error-empty", "error-populated"]),
                                ("struct", &["absent", "empty", "populated", "error-absent", "error-empty", "error-populated"]),
                                ("union", &["absent", "text", "text-empty", "items", "items-empty", "record", "record-empty", "error-absent", "error-text", "error-items", "error-record", "error-record-empty"]),
                                ("text", &["absent", "empty", "populated"]),
                                ("blob", &["absent", "empty", "populated"]),
                            ];
                            let mut observations = #{String}::new();
                            for (operation, modes) in cases {
                                for mode in *modes {
                                    let request = #{Http}::Request::builder()
                                        .method("GET").uri(format!("/{operation}?case={mode}"))
                                        .header("accept", "*/*")
                                        .body(#{Server}::body::to_boxed_sync(#{Vec}::<u8>::new())).unwrap();
                                    let response = service.clone().oneshot(request).await.unwrap();
                                    let status = response.status().as_u16();
                                    let mut headers: #{Vec}<_> = response.headers().iter()
                                        .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec())).collect();
                                    // Preserve the order of repeated header values while ignoring
                                    // the map's ordering between different header names.
                                    headers.sort_by(|left, right| left.0.cmp(&right.0));
                                    let bytes = response.into_body().collect().await.unwrap().to_bytes();
                                    assert_eq!(status >= 400, mode.starts_with("error"), "{operation}/{mode} must exercise its handler");
                                    if let #{Some}((_, length)) = headers.iter().find(|(name, _)| name == "content-length") {
                                        assert_eq!(::std::str::from_utf8(length).unwrap().parse::<usize>().unwrap(), bytes.len());
                                    }
                                    // JSON field order has no semantic meaning. Preserve all HTTP
                                    // headers (including Content-Length), status, and non-JSON bytes.
                                    let body = match #{Json}::from_slice::<#{Json}::Value>(&bytes) {
                                        #{Ok}(value) => format!("json:{value}"),
                                        #{Err}(_) => format!("bytes:{bytes:?}"),
                                    };
                                    observations.push_str(&format!("{operation}/{mode}\t{status}\t{headers:?}\t{body}\n"));
                                }
                            }
                            ::std::fs::write("${snapshot.toAbsolutePath()}", observations).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                            "Json" to CargoDependency.SerdeJson.toType(),
                        )
                    }
                }
            }
        }
        Files.readString(observations.resolve("true.txt")) shouldBe Files.readString(observations.resolve("false.txt"))
    }
}
