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

internal class ServerSchemaMalformedHeadersTest {
    @Test
    fun `malformed modeled headers never reach generated handlers`() {
        for ((protocol, contentType) in listOf("restJson1" to "application/json", "restXml" to "application/xml")) {
            val model =
                """
                ${'$'}version: "2"
                namespace test.malformedheaders
                use aws.protocols#$protocol
                @$protocol
                service Headers { version: "1", operations: [Read] }
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
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode(),
                    cargoCommand = "cargo test --quiet",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("raw_modeled_headers_are_validated_before_the_handler") {
                        rustTemplate(
                            """
                            use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
                            let calls = Arc::new(AtomicUsize::new(0));
                            let counter = calls.clone();
                            let config = crate::service::HeadersConfig::builder().build();
                            let service = crate::service::Headers::builder(config)
                                .read(move |_: crate::input::ReadInput| {
                                    counter.fetch_add(1, Ordering::SeqCst);
                                    async { #{Ok}::<_, crate::error::ReadError>(crate::output::ReadOutput::builder().build()) }
                                })
                                .build_unchecked();
                            for name in ["x-optional", "x-required", "x-list", "x-dates", "x-meta-key"] {
                                let valid = if name == "x-dates" {
                                    b"Thu, 01 Jan 1970 00:00:00 GMT".as_slice()
                                } else { b"valid".as_slice() };
                                for values in [vec![b"\xff".as_slice()], vec![valid, b"\xff"], vec![b"\xff", valid]] {
                                    let mut request = #{Http}::Request::builder().method("GET").uri("/read")
                                        .header("content-type", "$contentType");
                                    if name != "x-required" { request = request.header("x-required", "valid"); }
                                    for value in values { request = request.header(name, #{Http}::HeaderValue::from_bytes(value).unwrap()); }
                                    let request = request.body(#{Server}::body::to_boxed_sync("")).unwrap();
                                    let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                    assert_eq!(response.status(), 400, "{name}");
                                    let kind = response.extensions().get::<#{Server}::extension::RuntimeErrorExtension>().unwrap();
                                    assert_eq!(kind.as_str(), "SerializationException", "{name}");
                                    assert_eq!(calls.load(Ordering::SeqCst), 0, "{name}");
                                }
                            }
                            // An unmodeled malformed header remains harmless.
                            let request = #{Http}::Request::builder().method("GET").uri("/read")
                                .header("content-type", "$contentType")
                                .header("x-required", "valid")
                                .header("x-unmodeled", #{Http}::HeaderValue::from_bytes(b"\xff").unwrap())
                                .body(#{Server}::body::to_boxed_sync("")).unwrap();
                            let response = #{Tower}::ServiceExt::oneshot(service, request).await.unwrap();
                            assert_eq!(response.status(), 200);
                            assert_eq!(calls.load(Ordering::SeqCst), 1);
                            """,
                            *preludeScope,
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                            "Tower" to ServerCargoDependency.Tower.toType(),
                        )
                    }
                }
            }
        }
    }
}
