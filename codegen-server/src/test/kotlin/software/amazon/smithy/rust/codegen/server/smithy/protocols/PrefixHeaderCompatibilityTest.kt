/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols

import org.junit.jupiter.api.Test
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

internal class PrefixHeaderCompatibilityTest {
    @Test
    fun `legacy prefix headers trim surrounding whitespace`() = checkHeaders(schemaSerde = false)

    @Test
    fun `schema prefix headers match legacy whitespace handling`() = checkHeaders(schemaSerde = true)

    @Test
    fun `legacy prefix headers honor media types`() = checkHeaders(schemaSerde = false, mediaTypes = true)

    @Test
    fun `schema prefix headers match legacy media types`() = checkHeaders(schemaSerde = true, mediaTypes = true)

    private fun checkHeaders(
        schemaSerde: Boolean,
        mediaTypes: Boolean = false,
    ) {
        for (protocol in listOf("restJson1", "restXml")) {
            val valueShape = if (mediaTypes) "JsonString" else "String"
            val model =
                """
                ${'$'}version: "2"
                namespace test.prefixwhitespace
                use aws.protocols#$protocol
                @$protocol
                service Headers { version: "1", operations: [Read] }
                @http(method: "GET", uri: "/read")
                operation Read {
                    input := {
                        @httpPrefixHeaders("X-Meta-") meta: Metadata
                        @httpHeader("x-scalar") scalar: $valueShape
                        @httpQueryParams query: Query
                    }
                    output := { @httpPayload value: String }
                }
                map Metadata { key: String, value: $valueShape }
                map Query { key: String, value: String }
                @mediaType("application/json")
                string JsonString
                """.asSmithyModel()
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("prefix_headers_trim_values_without_trimming_query_values") {
                        rustTemplate(
                            """
                            for (value, expected) in [${if (mediaTypes) {
                                """
                                ("dHJ1ZQ==", "true"),
                                ("  dHJ1ZQ==  ", "true"),
                                ("\"dHJ1ZQ==\"", "true"),
                                ("aMOpbGxv", "héllo"),
                                """
                            } else {
                                """
                                ("  padded  ", "padded"),
                                ("\t padded \t", "padded"),
                                ("", ""),
                                (" \t ", ""),
                                ("  \"quoted value\"  ", "\"quoted value\""),
                                ("  one,two  ", "one,two"),
                                ("  one  two\tthree  ", "one  two\tthree"),
                                (" \u{a0}padded\u{a0} ", "padded"),
                                """
                            }}
                            ] {
                                let config = crate::service::HeadersConfig::builder().build();
                                let service = crate::service::Headers::builder(config)
                                    .read(move |input: crate::input::ReadInput| {
                                        assert_eq!(input.scalar.as_deref(), #{Some}(expected));
                                        assert_eq!(input.meta, #{Some}(#{HashMap}::from([
                                            ("a".to_owned(), expected.to_owned()),
                                        ])));
                                        assert_eq!(input.query, #{Some}(#{HashMap}::from([
                                            ("query".to_owned(), "  query  ".to_owned()),
                                        ])));
                                        async move { crate::output::ReadOutput::builder().value(#{Some}(expected.to_owned())).build() }
                                    })
                                    .build_unchecked();
                                let request = #{Http}::Request::builder().method("GET").uri("/read?query=%20%20query%20%20")
                                    .header("x-meta-a", #{Http}::HeaderValue::from_bytes(value.as_bytes()).unwrap())
                                    .header("x-scalar", #{Http}::HeaderValue::from_bytes(value.as_bytes()).unwrap())
                                    .body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
                                let status = response.status().as_u16();
                                let headers = response.headers().clone();
                                let body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                                println!("$protocol schemaSerde=$schemaSerde mediaTypes=$mediaTypes {value:?}: {status} {headers:?} {body:?}");
                                assert_eq!(status, 200);
                                assert_eq!(headers["content-type"], "text/plain");
                                assert_eq!(headers["content-length"], expected.len().to_string());
                                assert_eq!(body.as_ref(), expected.as_bytes());
                            }
                            ${if (mediaTypes) {
                                """
                            use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
                            let calls = Arc::new(AtomicUsize::new(0));
                            let counter = calls.clone();
                            let config = crate::service::HeadersConfig::builder().build();
                            let service = crate::service::Headers::builder(config)
                                .read(move |_: crate::input::ReadInput| {
                                    counter.fetch_add(1, Ordering::SeqCst);
                                    async { crate::output::ReadOutput::builder().build() }
                                })
                                .build_unchecked();
                            for values in [
                                vec!["%%%"],
                                vec!["/w=="],
                                vec!["dHJ1ZQ==,dHJ1ZQ=="],
                                vec!["dHJ1ZQ==", "dHJ1ZQ=="],
                            ] {
                                let mut request = #{Http}::Request::builder().method("GET").uri("/read");
                                for value in &values { request = request.header("x-meta-a", *value); }
                                let request = request.body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let headers = response.headers().clone();
                                let body = #{BodyUtil}::BodyExt::collect(response.into_body()).await.unwrap().to_bytes();
                                println!("$protocol schemaSerde=$schemaSerde invalid {values:?}: {status} {headers:?} {body:?}");
                                assert_eq!(status, 400);
                                assert_eq!(headers["content-type"], ${if (protocol == "restJson1") "\"application/json\"" else "\"application/xml\""});
                                assert_eq!(body.as_ref(), b"{}");
                                if "$protocol" == "restJson1" {
                                    assert_eq!(headers["x-amzn-errortype"], "SerializationException");
                                }
                                assert_eq!(calls.load(Ordering::SeqCst), 0);
                            }
                                """
                            } else {
                                ""
                            }}
                            """,
                            *preludeScope,
                            "HashMap" to RuntimeType.HashMap,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                        )
                    }
                }
            }
        }
    }
}
