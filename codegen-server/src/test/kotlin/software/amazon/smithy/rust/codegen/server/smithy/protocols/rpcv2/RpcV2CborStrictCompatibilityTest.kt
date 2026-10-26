/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.protocols.rpcv2

import io.kotest.matchers.shouldBe
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

/**
 * Replays populated CBOR outputs, modeled errors, and event streams through independent legacy
 * and schema services. It validates decoded CBOR, full body consumption, content lengths, handler
 * coverage, and initial/event/exception frames. Complete recorded captures must match across
 * serde modes, and an optional historical capture can provide an additional baseline.
 */
internal class RpcV2CborStrictCompatibilityTest {
    @Test
    fun `populated payloads errors and stream frames match legacy without exclusions`() {
        // Keep the full trait case as the default. A clean historical generator
        // cannot compile HTTP-bound error headers; the explicit alternate model
        // allows the rest of the historical behavior to be observed separately.
        val bindErrorHeader = System.getenv("STRICT_CBOR_BIND_ERROR_HEADER") != "false"
        val model =
            """
            ${'$'}version: "2"
            namespace test.strictcbor
            use smithy.protocols#rpcv2Cbor
            @rpcv2Cbor
            service StrictCbor { version: "1", operations: [Structured, Unioned, Plain, Download] }
            @http(method: "POST", uri: "/structured", code: 201)
            operation Structured {
                input := { @httpPayload body: Target, @httpHeader("x-mode") mode: String }
                output := { @httpPayload body: Target, @httpHeader("x-mode") mode: String }
                errors: [Failure]
            }
            @http(method: "POST", uri: "/unioned")
            operation Unioned {
                input := { @httpPayload body: Choice }
                output := { @httpPayload body: Choice }
            }
            operation Plain {
                input := { body: Target, mode: String }
                output := { body: Target, mode: String }
                errors: [Failure]
            }
            structure Target {
                child: String
                children: Targets
                tags: Tags
                counts: Counts
                choice: Choice
                empty: Empty
                flag: Boolean
                number: Long
            }
            structure Empty {}
            list Targets { member: Target }
            list Tags { member: String }
            map Counts { key: String, value: Integer }
            union Choice { text: String, nested: Target, tags: Tags, empty: Empty }
            @error("client") @httpError(409)
            structure Failure { @httpPayload body: Target ${if (bindErrorHeader) ", @httpHeader(\"x-failure\") marker: String" else ""} }
            operation Download {
                input := {}
                output := { topic: String, events: Events }
            }
            @streaming union Events { item: Item, empty: Empty, failure: StreamFailure }
            structure Item { @eventHeader name: String, @eventPayload body: Target }
            @error("server") structure StreamFailure { message: String, details: Target }
            """.asSmithyModel()
        val snapshots = Files.createTempDirectory("smithy-strict-cbor-header$bindErrorHeader-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = snapshots.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings =
                        ServerAdditionalSettings.builder().schemaSerde(schemaSerde).alwaysSendEventStreamInitialResponse().toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                crate.testModule {
                    tokioTest("replay_populated_requests_and_outputs") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            use #{BodyUtil}::BodyExt;
                            use std::sync::{Arc, Mutex};

                            // Map ordering and definite/indefinite encoding may differ. Preserve
                            // every entry and reject duplicates before sorting; do not decode to
                            // a JSON object, which could hide duplicate or missing keys.
                            fn canonical(value: #{Ciborium}::value::Value) -> #{String} {
                                use #{Ciborium}::value::Value;
                                match value {
                                    Value::Map(entries) => {
                                        let mut seen = std::collections::BTreeSet::new();
                                        let mut items = #{Vec}::new();
                                        for (key, value) in entries {
                                            let key = canonical(key);
                                            assert!(seen.insert(key.clone()), "duplicate CBOR map key: {key}");
                                            items.push(format!("{key}:{}", canonical(value)));
                                        }
                                        items.sort();
                                        format!("{{{}}}", items.join(","))
                                    }
                                    Value::Array(items) => format!("[{}]", items.into_iter().map(canonical).collect::<#{Vec}<_>>().join(",")),
                                    Value::Tag(tag, value) => format!("tag({tag},{})", canonical(*value)),
                                    value => format!("{value:?}"),
                                }
                            }
                            fn document(bytes: &[u8]) -> #{String} {
                                let mut cursor = std::io::Cursor::new(bytes);
                                let value: #{Ciborium}::value::Value =
                                    #{Ciborium}::de::from_reader(&mut cursor).expect("valid complete CBOR document");
                                assert_eq!(cursor.position() as usize, bytes.len(), "trailing CBOR bytes");
                                canonical(value)
                            }
                            fn request_bytes(json: &str) -> #{Vec}<u8> {
                                let value: #{Json}::Value = #{Json}::from_str(json).unwrap();
                                let mut bytes = #{Vec}::new();
                                #{Ciborium}::ser::into_writer(&value, &mut bytes).unwrap();
                                bytes
                            }
                            fn target() -> crate::model::Target {
                                crate::model::Target {
                                    child: #{Some}("event-child".into()),
                                    tags: #{Some}(vec!["one".into(), "two".into()]),
                                    counts: #{Some}([("negative".into(), -7), ("zero".into(), 0)].into_iter().collect()),
                                    choice: #{Some}(crate::model::Choice::Text("choice".into())),
                                    empty: #{Some}(crate::model::Empty {}),
                                    flag: #{Some}(false),
                                    number: #{Some}(-9),
                                    children: #{None},
                                }
                            }
                            fn choice_record(value: &crate::model::Choice) -> #{String} {
                                match value {
                                    crate::model::Choice::Text(value) => format!("text {value:?}"),
                                    crate::model::Choice::Nested(value) => format!("nested {}", target_record(value)),
                                    crate::model::Choice::Tags(value) => format!("tags {value:?}"),
                                    crate::model::Choice::Empty(value) => format!("empty {value:?}"),
                                }
                            }
                            fn target_record(value: &crate::model::Target) -> #{String} {
                                // Rust HashMap iteration is randomized independently in each
                                // generated crate; compare all entries in sorted order.
                                let counts = value.counts.as_ref().map(|counts| {
                                    let mut entries = counts.iter().collect::<#{Vec}<_>>();
                                    entries.sort();
                                    entries
                                });
                                format!("child={:?} children={:?} tags={:?} counts={:?} choice={:?} empty={:?} flag={:?} number={:?}",
                                    value.child,
                                    value.children.as_ref().map(|values| values.iter().map(target_record).collect::<#{Vec}<_>>()),
                                    value.tags, counts, value.choice.as_ref().map(choice_record),
                                    value.empty, value.flag, value.number)
                            }
                            let calls = Arc::new(Mutex::new(#{Vec}::new()));
                            let structured_calls = calls.clone();
                            let union_calls = calls.clone();
                            let plain_calls = calls.clone();
                            let service = crate::StrictCbor::builder(crate::StrictCborConfig::builder().build())
                                .structured(move |input: crate::input::StructuredInput| {
                                    structured_calls.lock().unwrap().push(format!("structured body={:?} mode={:?}",
                                        input.body.as_ref().map(target_record), input.mode));
                                    async move {
                                        if input.mode.as_deref() == #{Some}("fail") {
                                            #{Err}(crate::error::StructuredError::Failure(crate::error::Failure {
                                                body: input.body, ${if (bindErrorHeader) "marker: #{Some}(\"modeled-error\".into())," else ""}
                                            }))
                                        } else {
                                            #{Ok}(crate::output::StructuredOutput { body: input.body, mode: input.mode })
                                        }
                                    }
                                })
                                .unioned(move |input: crate::input::UnionedInput| {
                                    union_calls.lock().unwrap().push(format!("unioned body={:?}",
                                        input.body.as_ref().map(choice_record)));
                                    async move { crate::output::UnionedOutput { body: input.body } }
                                })
                                .plain(move |input: crate::input::PlainInput| {
                                    plain_calls.lock().unwrap().push(format!("plain body={:?} mode={:?}",
                                        input.body.as_ref().map(target_record), input.mode));
                                    async move {
                                        if input.mode.as_deref() == #{Some}("fail") {
                                            #{Err}(crate::error::PlainError::Failure(crate::error::Failure {
                                                body: input.body, ${if (bindErrorHeader) "marker: #{Some}(\"modeled-error\".into())," else ""}
                                            }))
                                        } else {
                                            #{Ok}(crate::output::PlainOutput { body: input.body, mode: input.mode })
                                        }
                                    }
                                })
                                .download(|_: crate::input::DownloadInput| async {
                                    #{Ok}::<_, crate::error::DownloadError>(crate::output::DownloadOutput {
                                        topic: #{Some}("populated-initial-response".into()),
                                        events: #{Futures}::stream::iter([
                                            #{Ok}(crate::model::Events::Item(crate::model::Item { name: #{Some}("event-header".into()), body: #{Some}(target()) })),
                                            #{Ok}(crate::model::Events::Empty(crate::model::Empty {})),
                                            #{Err}(crate::error::EventsError::StreamFailure(crate::error::StreamFailure {
                                                message: #{Some}("stream-failed".into()), details: #{Some}(target()),
                                            })),
                                        ]).into(),
                                    })
                                })
                                .build().unwrap();
                            let populated = r##"{"body":{"child":"value","children":[{"child":"nested"},{"empty":{}}],"tags":["first","second"],"counts":{"negative":-7,"zero":0,"positive":23},"choice":{"nested":{"child":"union-child"}},"empty":{},"flag":false,"number":-9},"mode":"success"}"##;
                            let mut cases = #{Vec}::new();
                            for operation in ["Structured", "Plain"] {
                                for json in [
                                    populated,
                                    r##"{}"##,
                                    r##"{"body":{}}"##,
                                    r##"{"body":{"tags":[],"counts":{},"children":[]},"mode":""}"##,
                                    r##"{"body":{"child":"failure","tags":["error"]},"mode":"fail"}"##,
                                    r##"{"mode":"fail"}"##,
                                ] {
                                    cases.push((operation, json));
                                }
                            }
                            for json in [
                                r##"{}"##,
                                r##"{"body":{"text":"text-value"}}"##,
                                r##"{"body":{"nested":{"child":"nested-value","tags":["a","b"]}}}"##,
                                r##"{"body":{"tags":["a","b"]}}"##,
                                r##"{"body":{"empty":{}}}"##,
                            ] {
                                cases.push(("Unioned", json));
                            }
                            cases.push(("Download", "{}"));
                            let mut records = #{String}::new();
                            for (operation, json) in cases {
                                let bytes = request_bytes(json);
                                let request = #{Http}::Request::builder()
                                    .method("POST")
                                    .uri(format!("/service/StrictCbor/operation/{operation}"))
                                    .header("content-type", "application/cbor")
                                    .header("smithy-protocol", "rpc-v2-cbor")
                                    .header("accept", if operation == "Download" {
                                        "application/vnd.amazon.eventstream, application/cbor"
                                    } else { "application/cbor" })
                                    // RPC must use body fields even when HTTP binding traits exist.
                                    .header("x-mode", "fail")
                                    .body(#{Server}::body::to_boxed_sync(bytes)).unwrap();
                                let response = service.clone().oneshot(request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers = response.headers().iter()
                                    .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec()))
                                    .collect::<#{Vec}<_>>();
                                headers.sort();
                                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                                if let #{Some}((_, length)) = headers.iter().find(|(name, _)| name == "content-length") {
                                    assert_eq!(std::str::from_utf8(length).unwrap().parse::<usize>().unwrap(), bytes.len());
                                }
                                // Validate length against each actual encoding, then compare the
                                // decoded document; CBOR permits both map encodings.
                                headers.retain(|(name, _)| name != "content-length");
                                let body = if operation == "Download" {
                                    let mut remaining = bytes.as_ref();
                                    let mut frames = #{Vec}::new();
                                    while !remaining.is_empty() {
                                        let before = remaining.len();
                                        let frame = #{EventStream}::frame::read_message_from(&mut remaining).expect("valid frame with CRCs");
                                        assert!(remaining.len() < before);
                                        let mut headers = frame.headers().iter()
                                            .map(|header| (header.name().as_str().to_owned(), format!("{:?}", header.value())))
                                            .collect::<#{Vec}<_>>();
                                        headers.sort();
                                        let payload = document(frame.payload());
                                        frames.push(format!("{headers:?} {payload}"));
                                    }
                                    assert_eq!(frames.len(), 4, "initial metadata, populated event, empty event and modeled exception must all reach the wire");
                                    frames.join("\n")
                                } else { document(&bytes) };
                                records.push_str(&format!("{operation} {json}\n{status} {headers:?}\n{body}\n{:?}\n", calls.lock().unwrap()));
                            }
                            assert_eq!(calls.lock().unwrap().len(), 17, "every non-stream case reached its handler");
                            std::fs::write("$snapshot", records).unwrap();
                            """,
                            *preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Ciborium" to CargoDependency.Ciborium.toDevDependency().toType(),
                            "Json" to CargoDependency.SerdeJson.toDevDependency().toType(),
                            "Futures" to CargoDependency.FuturesUtil.toType(),
                            "EventStream" to CargoDependency.smithyEventStream(context.runtimeConfig).toType(),
                        )
                    }
                }
            }
        }
        Files.readString(snapshots.resolve("true.txt")) shouldBe Files.readString(snapshots.resolve("false.txt"))
        System.getenv("STRICT_CBOR_HISTORICAL_BASELINE")?.let { historical ->
            Files.readString(snapshots.resolve("true.txt")) shouldBe Files.readString(java.nio.file.Paths.get(historical))
        }
    }
}
