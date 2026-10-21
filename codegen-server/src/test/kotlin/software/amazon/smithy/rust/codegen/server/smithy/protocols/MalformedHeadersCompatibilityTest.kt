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

internal class MalformedHeadersCompatibilityTest {
    @Test
    fun `REST JSON malformed headers match legacy wire responses`() = compare("restJson1")

    @Test
    fun `REST XML malformed headers match legacy wire responses`() = compare("restXml")

    private fun compare(protocol: String) {
        val run = Files.createTempDirectory("malformed-headers-$protocol-")
        val contentType = if (protocol == "restXml") "application/xml" else "application/json"
        val model =
            """
            ${'$'}version: "2"
            namespace test.malformedheaders
            use aws.protocols#$protocol
            @$protocol service Headers { version: "1", operations: [Read] }
            @http(uri: "/read", method: "GET")
            operation Read {
                input := {
                    @httpHeader("x-optional") optional: String
                    @required @httpHeader("x-required") required: String
                    @httpHeader("x-list") list: Strings
                    @httpHeader("x-dates") dates: Dates
                    @httpPrefixHeaders("x-meta-") meta: Metadata
                }
            }
            list Strings { member: String }
            list Dates { member: Timestamp }
            map Metadata { key: String, value: String }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                check(context.usesSchemaHttpSerde == schemaSerde)
                crate.testModule {
                    tokioTest("malformed_header_observations") {
                        rustTemplate(
                            """
                            use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
                            let calls = Arc::new(AtomicUsize::new(0));
                            let counter = calls.clone();
                            let app = crate::Headers::builder(crate::HeadersConfig::builder().build())
                                .read(move |_: crate::input::ReadInput| {
                                    counter.fetch_add(1, Ordering::SeqCst);
                                    async { #{Ok}::<_, crate::error::ReadError>(crate::output::ReadOutput::builder().build()) }
                                }).build_unchecked();
                            let mut cases = #{Vec}::new();
                            for name in ["x-optional", "x-required", "x-list", "x-dates", "x-meta-key"] {
                                let valid = if name == "x-dates" {
                                    b"Thu, 01 Jan 1970 00:00:00 GMT".as_slice()
                                } else { b"valid".as_slice() };
                                for (order, values) in [
                                    ("invalid", vec![b"\xff".as_slice()]),
                                    ("valid-invalid", vec![valid, b"\xff"]),
                                    ("invalid-valid", vec![b"\xff".as_slice(), valid]),
                                ] {
                                    cases.push((format!("{name}/{order}"), name, values));
                                }
                            }
                            cases.extend([
                                ("unmodeled-invalid".into(), "x-unmodeled", vec![b"\xff".as_slice()]),
                                ("invalid-date".into(), "x-dates", vec![b"not-a-date".as_slice()]),
                                ("unterminated-list".into(), "x-list", vec![b"\"unterminated".as_slice()]),
                                ("repeated-scalar".into(), "x-optional", vec![b"one".as_slice(), b"two".as_slice()]),
                                ("repeated-prefix".into(), "x-meta-key", vec![b"one".as_slice(), b"two".as_slice()]),
                                ("quoted-list".into(), "x-list", vec![b"\"one,two\", three".as_slice()]),
                            ]);
                            let mut records = #{String}::new();
                            for (case, name, values) in cases {
                                calls.store(0, Ordering::SeqCst);
                                let mut request = #{Http}::Request::get("/read").header("content-type", "$contentType");
                                if name != "x-required" { request = request.header("x-required", "valid"); }
                                for value in values {
                                    request = request.header(name, #{Http}::HeaderValue::from_bytes(value).unwrap());
                                }
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(),
                                    request.body(#{Server}::body::empty_sync()).unwrap()).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                let kind = response.extensions().get::<#{Server}::extension::RuntimeErrorExtension>()
                                    .map(|kind| kind.as_str().to_owned());
                                #{Collect}
                                // Pinned after replaying these requests with schemaSerde=false.
                                // Generated serde XML serialization rejections also have the literal body {}.
                                if ${!schemaSerde} {
                                    let success = matches!(case.as_str(), "unmodeled-invalid" | "quoted-list");
                                    assert_eq!(status, if success { 200 } else { 400 }, "{case}");
                                    assert_eq!(calls.load(Ordering::SeqCst), usize::from(success), "{case}");
                                    assert_eq!(kind.as_deref(), if success { #{None} } else { #{Some}("SerializationException") }, "{case}");
                                    assert_eq!(body.as_ref(), if success { b"".as_slice() } else { b"{}".as_slice() }, "{case}");
                                    let mut expected_headers = #{Vec}::new();
                                    if success { expected_headers.push(("content-length".to_owned(), b"0".to_vec())); }
                                    if !success || ${protocol == "restJson1"} {
                                        expected_headers.push(("content-type".to_owned(), b"$contentType".to_vec()));
                                    }
                                    if !success && ${protocol == "restJson1"} {
                                        expected_headers.push(("x-amzn-errortype".to_owned(), b"SerializationException".to_vec()));
                                    }
                                    expected_headers.sort();
                                    assert_eq!(headers, expected_headers, "{case}");
                                }
                                records.push_str(&format!("{case}: {status} calls={} kind={kind:?} headers={headers:?} body={:?}\n",
                                    calls.load(Ordering::SeqCst), body.as_ref()));
                            }
                            std::fs::write(${snapshot.toString().dq()}, records).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "Collect" to ServerHttpTestHelpers.httpBodyToBytes(context.runtimeConfig, "body", "response"),
                        )
                    }
                }
            }
        }
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}
