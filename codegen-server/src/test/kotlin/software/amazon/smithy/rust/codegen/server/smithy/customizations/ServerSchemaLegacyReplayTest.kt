/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files

/** Replays identical request bytes through separately generated legacy and schema servers. */
internal class ServerSchemaLegacyReplayTest {
    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "restXml", "awsJson1_0", "awsJson1_1"])
    fun `default schema responses match legacy responses`(protocol: String) {
        val xml = protocol == "restXml"
        val rpc = protocol.startsWith("awsJson")
        val contentType =
            when (protocol) {
                "restXml" -> "application/xml"
                "awsJson1_0" -> "application/x-amz-json-1.0"
                "awsJson1_1" -> "application/x-amz-json-1.1"
                else -> "application/json"
            }
        val model =
            """
            ${'$'}version: "2"
            namespace test.replay
            use aws.protocols#$protocol
            @$protocol
            service Replay { version: "1", operations: [Echo, Events] }
            @http(method: "POST", uri: "/echo/{key}", code: 201)
            operation Echo { input: EchoInput, output: EchoOutput, errors: [smithy.framework#ValidationException, Failure] }
            structure EchoInput {
                @required @httpLabel key: String
                ${if (xml) "" else "@required"} value: ${if (xml) "String" else "ShortText"}
                tags: Tags
                later: ${if (xml) "String" else "ShortText"}
                ${if (rpc) "" else "@httpHeader(\"x-count\")"} count: Integer
            }
            @length(min: 2) string ShortText
            list Tags { member: String }
            structure EchoOutput {
                count: Integer, tags: Tags, value: String
                @httpHeader("content-type") media: String
            }
            @error("client") @httpError(409) structure Failure { message: String, @httpHeader("x-amzn-errortype") kind: String }
            @http(method: "POST", uri: "/events")
            operation Events { input := {}, output := { @httpPayload events: Stream } }
            @streaming union Stream { item: Item, header: HeaderItem, text: TextItem, binary: BinaryItem, empty: EmptyItem, failure: Failure }
            structure HeaderItem { @eventHeader name: String, extra: String }
            structure TextItem { @eventPayload value: String }
            structure BinaryItem { @eventPayload value: Blob }
            structure EmptyItem {}
            structure Item { value: String }
        """.asSmithyModel()
        val snapshots = Files.createTempDirectory("smithy-legacy-replay-")
        for (schema in listOf(false, true)) {
            val snapshot = snapshots.resolve("$schema.txt")
            val bodies =
                if (xml) {
                    listOf(
                        "<EchoInput><value>ok</value><tags><member>a</member><wrong>b</wrong></tags></EchoInput>",
                        "<EchoInput><value>ok</value></EchoInput>junk",
                        "<EchoInput><value>ok</value></EchoInput><EchoInput/>",
                        "<EchoInput><value>ok</value></EchoInput></EchoInput>",
                        "<EchoInput><value>ok</value>",
                        "<EchoInput><value>ok</wrong></EchoInput>",
                        "<EchoInput><value>ok</val",
                        "<Wrong><value>ok</value></Wrong>",
                        "<EchoInput><value>x</value></EchoInput>",
                        "<EchoInput/>",
                        "<EchoInput><value>fail</value></EchoInput>",
                    )
                } else {
                    listOf(
                        """{"key":"k","value":"ok","tags":["a"]}""",
                        """{"key":"k","value":"ok","unknown":["\i"]}""",
                        """{"key":"k","value":"ok","unknown":{"\i":"\u12"}}""",
                        """{"key":"k","value":"ok","unknown":"\udee9"}""",
                        """{"key":"k","value":"\i"}""",
                        """{"key":"k","value":"x"}""",
                        """{"key":"k"}""",
                        """{"key":"k","value":"fail"}""",
                        """{"key":"k","value":"ok"}junk""",
                        """{"key":"k","value":"ok","count":1.0}""",
                        """{"key":"k","value":"ok","count":1.5}""",
                        """{"key":"k","value":"ok","count":2147483648.0}""",
                        """{"key":"k","value":"ok","tags":[null]}""",
                        """{"key":"k","value":"ok","tags":[1]}""",
                        """{"key":"k","value":"ok","value":"last"}""",
                        """{"key":"k","value":"x","later":"x"}""",
                        """{"key":"k","later":"x"}""",
                        """{"key":"k","value":"x","count":"bad"}""",
                        """{"key":"k","value":"ok","unknown":[1,]}""",
                        "{\"key\":\"k\",\"value\":\"ok\",\"unknown\":\"raw\ncontrol\"}",
                    )
                }
            val bodyLiterals = bodies.joinToString(",\n") { "r##\"$it\"##" }
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schema}}""").expectObjectNode()),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("exact_request_replays") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            use #{BodyUtil}::BodyExt;
                            use std::sync::{Arc, Mutex};
                            let calls = Arc::new(Mutex::new(#{Vec}::new()));
                            let counter = calls.clone();
                            let service = crate::Replay::builder(crate::ReplayConfig::builder().build())
                                .echo(move |input: crate::input::EchoInput| {
                                    counter.lock().unwrap().push(format!("{:?}", input));
                                    async move {
                                        if ${if (xml) "input.value.as_deref() == #{Some}(\"fail\")" else "input.value.as_str() == \"fail\""} {
                                            return #{Err}(crate::error::EchoError::Failure(crate::error::Failure { message: #{Some}("failure".into()), kind: #{Some}("custom-error".into()) }));
                                        }
                                        #{Ok}(crate::output::EchoOutput { value: ${if (xml) "input.value" else "#{Some}(input.value.as_str().to_owned())"}, tags: input.tags, count: input.count, media: #{Some}("application/test".into()) })
                                    }
                                })
                                .events(|_: crate::input::EventsInput| async {
                                    #{Ok}::<_, crate::error::EventsError>(crate::output::EventsOutput { events: #{Futures}::stream::iter([
                                        #{Ok}(crate::model::Stream::Item(crate::model::Item { value: #{Some}("event".into()) })),
                                        #{Ok}(crate::model::Stream::Header(crate::model::HeaderItem { name: #{Some}("header".into()), extra: #{Some}("extra".into()) })),
                                        #{Ok}(crate::model::Stream::Text(crate::model::TextItem { value: #{Some}("text".into()) })),
                                        #{Ok}(crate::model::Stream::Binary(crate::model::BinaryItem { value: #{Some}(#{Types}::Blob::new(b"binary")) })),
                                        #{Ok}(crate::model::Stream::Empty(crate::model::EmptyItem {})),
                                        #{Err}(crate::error::StreamError::Failure(crate::error::Failure { message: #{Some}("failure".into()), kind: #{Some}("custom-error".into()) })),
                                    ]).into() })
                                })
                                .build().unwrap();
                            let mut requests = #{Vec}::new();
                            for body in [$bodyLiterals] {
                                requests.push(("POST", "${if (rpc) "/" else "/echo/k"}", "Replay.Echo", "$contentType", "*/*", "", body.as_bytes().to_vec()));
                            }
                            for (ct, accept, count, body) in [
                                ("$contentType", "application/unsupported", "bad", "malformed"),
                                ("text/plain", "*/*", "bad", "malformed"),
                                ("text/plain", "*/*", "", ""),
                                ("text/xml", "*/*", "", ""),
                                ("", "*/*", "", ""),
                                ("$contentType", "*/*", "bad", "malformed"),
                                ("$contentType", "*/*", "bad", ${if (xml) "r##\"<EchoInput><value>x</value></EchoInput>\"##" else "r##\"{\"key\":\"k\",\"value\":\"x\"}\"##"}),
                            ] {
                                requests.push(("POST", "${if (rpc) "/" else "/echo/k"}", "Replay.Echo", ct, accept, count, body.as_bytes().to_vec()));
                            }
                            requests.extend([
                                ("GET", "/missing", "Replay.Missing", "$contentType", "*/*", "", ""),
                                ("PUT", "/echo/k", "Replay.Echo", "$contentType", "*/*", "", ""),
                                ("POST", "${if (rpc) "/" else "/events"}", "Replay.Events", "$contentType", "*/*", "", ""),
                            ].map(|(method, uri, target, ct, accept, count, body)| (method, uri, target, ct, accept, count, body.as_bytes().to_vec())));
                            ${if (!xml) {
                                """
                            requests.push(("POST", "${if (rpc) "/" else "/echo/k"}", "Replay.Echo", "$contentType", "*/*", "",
                                b"{\"key\":\"k\",\"value\":\"ok\",\"unknown\":\"\xff\"}".to_vec()));
                            """
                            } else {
                                ""
                            }}
                            let mut snapshot = #{String}::new();
                            for (method, uri, target, ct, accept, count, body) in requests {
                                let mut request = #{Http}::Request::builder().method(method).uri(uri)
                                    .header("x-amz-target", target).header("accept", accept);
                                if !ct.is_empty() { request = request.header("content-type", ct); }
                                if !count.is_empty() { request = request.header("x-count", count); }
                                let request = request.body(#{Server}::body::to_boxed_sync(body.clone())).unwrap();
                                let response = service.clone().oneshot(request).await.unwrap();
                                let status = response.status();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                                // JSON object order is not part of the wire contract. Keep every
                                // request byte, header, status, frame and parsed handler input exact.
                                let bytes = if let #{Ok}(json) = #{Json}::from_slice::<#{Json}::Value>(&bytes) {
                                    #{Json}::to_vec(&json).unwrap().into()
                                } else { bytes };
                                snapshot.push_str(&format!("{method} {uri} {ct} {accept} {count} {body:?}\n{status} {headers:?}\n{bytes:?}\n{:?}\n", calls.lock().unwrap()));
                            }
                            std::fs::write("$snapshot", snapshot).unwrap();
                            """,
                            *preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.Tower.toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Futures" to CargoDependency.FuturesUtil.toType(),
                            "Json" to CargoDependency.SerdeJson.toType(),
                            "Types" to RuntimeType.smithyTypes(context.runtimeConfig),
                        )
                    }
                }
            }
        }
        Files.readString(snapshots.resolve("true.txt")) shouldBe Files.readString(snapshots.resolve("false.txt"))
    }
}
