/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.customizations

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
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

internal class ServerSchemaRequestStrictnessTest {
    @Test
    fun `malformed requests never reach generated handlers`() {
        for ((protocol, namespace, contentType) in listOf(
            Triple("restJson1", "aws.protocols", "application/json"),
            Triple("restXml", "aws.protocols", "application/xml"),
            Triple("rpcv2Cbor", "smithy.protocols", "application/cbor"),
        )) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.strictness
                use $namespace#$protocol
                @$protocol
                service StrictService { version: "1", operations: [Read${if (protocol == "restXml") {
                    ", Payload"
                } else if (protocol == "restJson1") {
                    ", Greedy"
                } else {
                    ""
                }}] }
                @http(uri: "/read", method: "POST")
                operation Read { input: Request, output: Response }
                @xmlName("WireRequest")
                structure Request {
                    @default(7)
                    value: Long
                    choice: Choice
                    @httpHeader("x-epoch") @timestampFormat("epoch-seconds") epoch: Timestamp
                    @httpHeader("x-date") date: Timestamp
                    @xmlName("Renamed")
                    child: Child
                    @httpHeader("x-count")
                    count: Integer
                    @httpHeader("x-enabled")
                    enabled: Boolean
                    @httpHeader("x-text")
                    text: String
                    @httpPrefixHeaders("x-meta-")
                    meta: Metadata
                }
                ${if (protocol == "restXml") {
                    """
                    @http(uri: "/payload", method: "POST")
                    operation Payload { input := { @httpPayload @xmlName("MemberRoot") data: Data }, output: Response }
                    @xmlName("TargetRoot")
                    structure Data { @xmlName("Renamed") child: Child }
                """
                } else {
                    ""
                }}
                structure Child { text: String, @default(true) flag: Boolean }
                union Choice { text: String, number: Integer }
                map Metadata { key: String, value: String }
                structure Response {}
                ${if (protocol == "restJson1") {
                    """
                    @http(uri: "/greedy/{key+}", method: "GET")
                    operation Greedy { input := { @required @httpLabel key: String }, output: Response }
                """
                } else {
                    ""
                }}
            """.asSmithyModel()
            val cases =
                when (protocol) {
                    "restJson1" ->
                        """
                    (b"".as_slice(), true),
                    (b" ", false),
                    (br#"{"choice":{"text":"a"}}"#, true),
                    (br#"{"choice":{"__type":"Choice","text":"a"}}"#, true),
                    (br#"{"choice":{"text":"a","number":1}}"#, false),
                    (br#"{"choice":{"text":"a","text":"b"}}"#, false),
                    (br#"{"choice":{"text":"a","z":1}}"#, false),
                    (br#"{"choice":{"z":1,"text":"a"}}"#, false),
                    (br#"{"choice":{"z":1}}"#, false),
                    (br#"{"value":1.0}"#, true),
                    (br#"{"value":9223372036854775807.0}"#, true),
                    (br#"{"value":9223372036854775808.0}"#, false),
                    (br#"{"value":1.5}"#, false),
                    (br#"{"value":12.}"#, false),
                    (b"{\"child\":", false),
                    (br#"{"unknown":[1,]}"#, false),
                    (b"{\"unknown\":\"raw\ncontrol\"}", false),
                    (b"{\"unknown\":\"\xff\"}", false),
                """
                    "restXml" ->
                        """
                    (b"<WireRequest><Renamed><text>ok</text></Renamed></WireRequest>".as_slice(), true),
                    (b"<Wrong><Renamed/></Wrong>", false),
                    (b"<WireRequest><value>7</value></WireRequest>", true),
                """
                    else ->
                        """
                    (b"\xa0".as_slice(), true),
                    (b"\xbf\xff", true),
                    (b"\xbf\x65child\xa0\xff", true),
                    (b"\xa1\x65value\xf6", true),
                    (b"\xa1\x65child\xa1\x64flag\xf6", true),
                    (b"\xa1\x66choice\xa1\x64text\x61a", true),
                    (b"\xa1\x66choice\xa2\x64text\x61a\x66number\x01", false),
                    (b"\xa1\x66choice\xa2\x64text\x61a\x61z\x01", false),
                    (b"\xa1\x66choice\xa1\x61z\x01", false),
                    (b"\xa1\x66choice\xa2\x61z\x01\x64text\x61a", false),
                    (b"\xa1\x66choice\xbf\x64text\x61a\x64text\x61b\xff", false),
                    (b"\xa1\x66choice\xbf\x64text\x61a\x61z\x01\xff", false),
                    (b"\xa0\x00", false),
                    (b"\xbf\xff\xff", false),
                """
                }
            val uri = if (protocol == "rpcv2Cbor") "/service/StrictService/operation/Read" else "/read"
            serverIntegrationTest(
                model,
                IntegrationTestParams(additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode()),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("request_validation_precedes_handler") {
                        rustTemplate(
                            """
                            use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
                            let calls = Arc::new(AtomicUsize::new(0));
                            let counter = calls.clone();
                            let config = crate::service::StrictServiceConfig::builder().build();
                            let service = crate::service::StrictService::builder(config)
                                .read(move |input: crate::input::ReadInput| {
                                    if let #{Some}(child) = input.child { assert!(child.flag); }
                                    counter.fetch_add(1, Ordering::SeqCst);
                                    async { crate::output::ReadOutput::builder().build() }
                                })
                                ${if (protocol == "restXml") ".payload(|_: crate::input::PayloadInput| async { crate::output::PayloadOutput::builder().build() })" else ""}
                                ${if (protocol == "restJson1") ".greedy(|input: crate::input::GreedyInput| async move { assert!([\"\", \"/a//b/\"].contains(&input.key.as_str())); #{Ok}::<_, crate::error::GreedyError>(crate::output::GreedyOutput::builder().build()) })" else ""}
                                .build_unchecked();
                            for (body, valid) in [${cases.replace("#", "##")}] {
                                let before = calls.load(Ordering::SeqCst);
                                let request = #{Http}::Request::builder().method("POST").uri("$uri")
                                    .header("content-type", "$contentType")
                                    .header("smithy-protocol", "rpc-v2-cbor")
                                    .body(#{Server}::body::to_boxed_sync(body.to_vec())).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                assert_eq!(response.status().is_success(), valid, "{body:?}");
                                assert_eq!(calls.load(Ordering::SeqCst) - before, usize::from(valid), "{body:?}");
                            }
                            ${if (protocol == "restXml") {
                                """
                            for (body, valid) in [
                                ("<MemberRoot><Renamed><text>ok</text></Renamed></MemberRoot>", true),
                                ("<TargetRoot/>", false),
                                ("<Wrong/>", false),
                            ] {
                                let request = #{Http}::Request::builder().method("POST").uri("/payload")
                                    .header("content-type", "$contentType")
                                    .body(#{Server}::body::to_boxed_sync(body)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                assert_eq!(response.status().is_success(), valid, "{body}");
                            }
                            """
                            } else {
                                ""
                            }}
                            ${if (protocol == "restJson1") {
                                """
                            for path in ["/greedy/", "/greedy//a//b/"] {
                                let request = #{Http}::Request::builder().method("GET").uri(path)
                                    .body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                assert!(response.status().is_success(), "{path}");
                            }
                            use #{Server}::schema::{ServerProtocol, DeserializableShape};
                            let protocol = #{Server}::protocol::rest_json_1::RestJson1Protocol::default();
                            for body in [
                                br##"{"text":"evil","count":99,"value":10}"##.as_slice(),
                                br##"{"text":false,"count":{},"value":10}"##,
                            ] {
                                let converted = #{RuntimeApi}::http::Request::try_from(
                                    #{Http}::Request::builder().method("POST").uri("/read")
                                        .header("content-type", "application/json")
                                        .header("x-text", "trusted").header("x-count", "7")
                                        .body(()).unwrap()).unwrap().into_parts();
                                let request = #{Server}::schema::ServerRequest { uri: converted.uri, headers: converted.headers, body: body.to_vec().into() };
                                let mut d = protocol.deserialize_request(crate::input::ReadInput::SCHEMA, &request).unwrap();
                                let input = crate::input::ReadInput::deserialize(&mut *d).unwrap();
                                assert_eq!(input.text.as_deref(), #{Some}("trusted"));
                                assert_eq!(input.count, #{Some}(7));
                                assert_eq!(input.value, 10);
                            }
                            for (headers, valid) in [
                                (vec![("x-count", ""), ("x-enabled", ""), ("x-text", "")], true),
                                (vec![("x-epoch", ""), ("x-date", "")], true),
                                (vec![("x-epoch", ""), ("x-epoch", "10")], true),
                                (vec![("x-date", ""), ("x-date", "Sun, 02 Jan 2000 20:34:56 GMT")], true),
                                (vec![("x-epoch", "10"), ("x-epoch", "20")], false),
                                (vec![("x-count", ""), ("x-count", "7,")], true),
                                (vec![("x-count", "7"), ("x-count", "8")], false),
                                (vec![("x-meta-color", "red"), ("x-meta-color", "blue")], false),
                            ] {
                                let before = calls.load(Ordering::SeqCst);
                                let mut request = #{Http}::Request::builder().method("POST").uri("/read");
                                for (name, value) in headers { request = request.header(name, value); }
                                let request = request.body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                assert_eq!(response.status().is_success(), valid);
                                assert_eq!(calls.load(Ordering::SeqCst) - before, usize::from(valid));
                            }
                            """
                            } else {
                                ""
                            }}
                            """,
                            *preludeScope,
                            "RuntimeApi" to RuntimeType.smithyRuntimeApi(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Tower" to ServerCargoDependency.Tower.toType(),
                        )
                    }
                }
            }
        }
    }
}
