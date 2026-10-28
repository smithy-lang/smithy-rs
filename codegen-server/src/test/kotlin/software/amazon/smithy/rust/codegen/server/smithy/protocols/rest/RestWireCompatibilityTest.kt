/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.rest

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.protocoltests.traits.AppliesTo
import software.amazon.smithy.protocoltests.traits.HttpRequestTestsTrait
import software.amazon.smithy.protocoltests.traits.HttpResponseTestsTrait
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.ServerAdditionalSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerHttpTestHelpers
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.readText

/** Models also run as standalone protocol tests in codegen-server-test, in both serde modes. */
private fun restWireModel(name: String): Model =
    Files.readString(Path.of("..", "codegen-server-test", "custom-test-models", "$name.smithy")).asSmithyModel()

/**
 * Reads request cases from the Smithy body-binding protocol fixtures and sends them through
 * legacy and schema REST JSON/XML servers. Body copies of label, header, and query members,
 * including incorrectly typed copies, must not overwrite the actual transport bindings.
 * It checks handler-derived values and compares exact status, all headers, and response bytes.
 */
internal class RestBodyBindingFilteringTest {
    @ParameterizedTest
    @ValueSource(strings = ["restJson1", "restXml"])
    fun `body copies of transport members preserve legacy bindings`(protocol: String) {
        val model = restWireModel("body-bindings-$protocol")
        val requests =
            model.expectShape(ShapeId.from("test.bodybindings#CreateOrder"))
                .expectTrait(HttpRequestTestsTrait::class.java).getTestCasesFor(AppliesTo.SERVER)
        val mediaType =
            if (protocol == "restJson1") {
                "application/json"
            } else {
                "application/xml"
            }
        val cases =
            requests.joinToString(",\n") {
                "(${it.body.get().dq()}, ${it.headers.containsKey("x-tenant-id")})"
            }
        val observations = Files.createTempDirectory("smithy-body-bindings-$protocol-")
        for (schemaSerde in listOf(false, true)) {
            val snapshot = observations.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib --tests",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("body_copies_cannot_assign_transport_members") {
                        rustTemplate(
                            """
                            use #{Tower}::ServiceExt;
                            use #{BodyUtil}::BodyExt;
                            let service = crate::Orders::builder(crate::OrdersConfig::builder().build())
                                .create_order(|input: crate::input::CreateOrderInput| async move {
                                    #{Ok}::<_, crate::error::CreateOrderError>(crate::output::CreateOrderOutput {
                                        value: #{Some}(format!("{}|{}|{}|{}|{}|{}",
                                            input.store_id,
                                            input.tenant_id.as_deref().unwrap_or("unset"),
                                            input.dry_run.unwrap_or(false),
                                            input.note.as_deref().unwrap_or("unset"),
                                            input.quantity.unwrap_or_default(),
                                            input.detail.and_then(|detail| detail.tenant_id).as_deref().unwrap_or("unset"),
                                        )),
                                    })
                                })
                                .build().unwrap();
                            let mut observations = #{String}::new();
                            for (body, with_header) in [$cases] {
                                let mut request = #{Http}::Request::builder()
                                    .method("POST").uri("/orders/store-7?dryRun=false")
                                    .header("content-type", ${mediaType.dq()});
                                if with_header {
                                    request = request.header("x-tenant-id", "tenant-A");
                                }
                                let request = request.body(#{Server}::body::to_boxed_sync(body.as_bytes().to_vec())).unwrap();
                                let response = service.clone().oneshot(request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(name, value)| (name.as_str().to_owned(), value.as_bytes().to_vec())).collect();
                                headers.sort();
                                let bytes = response.into_body().collect().await.unwrap().to_bytes();
                                assert_eq!(status, 201, "{body}");
                                let tenant = if with_header { "tenant-A" } else { "unset" };
                                assert_eq!(bytes.as_ref(), format!("store-7|{tenant}|false|hi|2|nested").as_bytes(), "{body}");
                                observations.push_str(&format!("{status} {headers:?} {bytes:?}\n"));
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, observations).unwrap();
                            """,
                            *RuntimeType.preludeScope,
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                            "BodyUtil" to CargoDependency.HttpBodyUtil01x.toType(),
                            "Http" to RuntimeType.http(context.runtimeConfig),
                            "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                        )
                    }
                }
            }
        }
        observations.resolve("true.txt").readText() shouldBe observations.resolve("false.txt").readText()
    }
}

/**
 * Echoes timestamp lists, maps, and nested collections through legacy and schema REST JSON/XML
 * servers. It checks the exact response text for member-level http-date, date-time, and epoch-seconds
 * formats and for unannotated protocol defaults, then compares status and all headers too.
 * Ordinary cases come from Smithy protocol-test traits; sparse timestamp null cases remain here
 * because the Smithy protocol-test validator rejects null timestamp parameters.
 */
internal class CollectionTimestampFormatCompatibilityTest {
    @Test
    fun `JSON collection timestamps preserve legacy formats`() = checkFormats("restJson1")

    @Test
    fun `XML collection timestamps preserve legacy formats`() = checkFormats("restXml")

    private fun checkFormats(protocol: String) {
        val xml = protocol == "restXml"
        val run = Files.createTempDirectory("collection-timestamp-parity-")
        // XML omits sparse timestamp maps because the legacy generator cannot compile them.
        val model = restWireModel("collection-timestamps-$protocol")
        val operation = model.expectShape(ShapeId.from("test.collectiontimestamps#Echo"))
        val requests = operation.expectTrait(HttpRequestTestsTrait::class.java).getTestCasesFor(AppliesTo.SERVER)
        val responses =
            operation.expectTrait(HttpResponseTestsTrait::class.java).getTestCasesFor(AppliesTo.SERVER).associateBy {
                it.id
            }
        // Smithy protocol test validation currently rejects null timestamp parameters even in
        // sparse collections. Keep those exact wire comparisons here without suppressing validation.
        val sparseNullCases =
            if (xml) {
                emptyList()
            } else {
                listOf(
                    """{"sparse":["Thu, 01 Jan 1970 00:00:00 GMT",null]}""",
                    """{"sparseMapping":{"one":null}}""",
                ).map { it to it }
            }
        val cases = requests.map { it.body.get() to responses.getValue(it.id).body.get() } + sparseNullCases
        // Only the serde setting changes between generated services; protocol and HTTP version match.
        for (schemaSerde in listOf(false, true)) {
            val snapshot = run.resolve("$schemaSerde.txt")
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().withHttp1x().schemaSerde(schemaSerde).toObjectNode(),
                    overrideTestDir =
                        run.resolve(
                            if (schemaSerde) {
                                "schema"
                            } else {
                                "legacy"
                            },
                        ).toFile(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Default,
            ) { context, crate ->
                crate.testModule {
                    tokioTest("collection_timestamp_formats") {
                        rustTemplate(
                            """
                            let app = crate::Times::builder(crate::TimesConfig::builder().build())
                                .echo(|input: crate::input::EchoInput| async move {
                                    crate::output::EchoOutput {
                                        dates: input.dates, mapping: input.mapping, iso: input.iso,
                                        epochs: input.epochs, nested: input.nested, sparse: input.sparse,
                                        defaults: input.defaults,
                                        nested_mapping: input.nested_mapping,
                                        ${if (xml) {
                                ""
                            } else {
                                "sparse_mapping: input.sparse_mapping,"
                            }}
                                    }
                                }).build().unwrap();
                            let cases = [
                                ${cases.joinToString(",\n") { (request, expected) -> "(${request.dq()}, ${expected.dq()})" }}
                            ];
                            let mut records = #{String}::new();
                            for (payload, expected) in cases {
                                let request = #{Http}::Request::post("/echo")
                                    .header("content-type", ${if (xml) {
                                "application/xml".dq()
                            } else {
                                "application/json".dq()
                            }})
                                    .body(#{Server}::body::to_boxed_sync(payload)).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(app.clone(), request).await.unwrap();
                                let status = response.status().as_u16();
                                let mut headers: #{Vec}<_> = response.headers().iter()
                                    .map(|(key, value)| (key.to_string(), value.to_str().unwrap().to_string())).collect();
                                headers.sort();
                                #{Collect}
                                let text = ::std::str::from_utf8(&body).unwrap();
                                println!("{status} {headers:?} {text}");
                                assert_eq!(status, 200);
                                assert_eq!(text, expected);
                                records.push_str(&format!("{status} {headers:?} {text}\n"));
                            }
                            ::std::fs::write(${snapshot.toString().dq()}, records).unwrap();
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
        // Compare status and all headers as well as bytes, catching content-length changes too.
        assertEquals(Files.readString(run.resolve("false.txt")), Files.readString(run.resolve("true.txt")), run.toString())
    }
}

/**
 * Reads header cases from the Smithy protocol fixtures and drives legacy and schema REST
 * JSON/XML services. It checks whitespace trimming and media-type decoding for scalar and prefix
 * headers while ensuring query values retain their whitespace. Successful responses must have
 * exact payload bytes, text content type, and content length. Repeated and invalid media-typed
 * headers must be rejected before the handler runs; repeated-header coverage stays in Rust here.
 */
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
            val model =
                restWireModel(
                    "prefix-headers-$protocol-${if (mediaTypes) {
                        "media"
                    } else {
                        "plain"
                    }}",
                )
            val requests =
                model.expectShape(ShapeId.from("test.prefixwhitespace#Read"))
                    .expectTrait(HttpRequestTestsTrait::class.java).getTestCasesFor(AppliesTo.SERVER)
            val cases =
                requests.joinToString(",\n") {
                    "(${it.headers.getValue("x-meta-a").dq()}, ${it.params.expectStringMember("scalar").value.dq()})"
                }
            serverIntegrationTest(
                model,
                IntegrationTestParams(
                    additionalSettings = ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
                    cargoCommand = "cargo test --quiet --lib -- --nocapture",
                ),
                testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
            ) { context, crate ->
                crate.testModule {
                    tokioTest("prefix_headers_trim_values_without_trimming_query_values") {
                        rustTemplate(
                            """
                            for (value, expected) in [$cases
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
                            // Protocol traits represent one value per header name; retain repeated-header coverage here.
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
                                assert_eq!(headers["content-type"], ${if (protocol == "restJson1") {
                                    "\"application/json\""
                                } else {
                                    "\"application/xml\""
                                }});
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
