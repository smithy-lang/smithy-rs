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
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

internal class ServerSchemaMediaTypeHeadersTest {
    @Test
    fun `media typed list headers round trip like legacy generated services`() {
        for (protocol in listOf("restJson1", "restXml")) {
            val contentType = if (protocol == "restJson1") "application/json" else "application/xml"
            for (schemaSerde in listOf(false, true)) {
                // Legacy media-type formatting passes enum/newtype wrappers directly to
                // base64::encode, which cannot compile. Compare plain strings against
                // legacy and exercise those wrappers in the schema path separately.
                val wrapperHeaders =
                    if (schemaSerde) {
                        """
                        @httpHeader("x-enums") enumItems: MediaEnums
                        @httpHeader("x-wrapped") wrappedItems: WrappedStrings
                        """
                    } else {
                        ""
                    }
                val wrapperAssertions =
                    if (schemaSerde) {
                        """
                        assert_eq!(input.enum_items.as_ref().unwrap().iter()
                            .map(|value| value.as_str()).collect::<#{Vec}<_>>(), ["object", "array"]);
                        assert_eq!(input.wrapped_items.as_ref().unwrap().iter()
                            .map(|value| value.as_str()).collect::<#{Vec}<_>>(), ["{}", "line\nbreak"]);
                        """
                    } else {
                        ""
                    }
                val wrapperOutput =
                    if (schemaSerde) {
                        """
                        .enum_items(input.enum_items)
                        .wrapped_items(input.wrapped_items)
                        """
                    } else {
                        ""
                    }
                val wrapperRequest =
                    if (schemaSerde) {
                        """
                        .header("x-enums", "b2JqZWN0")
                        .header("x-enums", "YXJyYXk=")
                        .header("x-wrapped", "e30=")
                        .header("x-wrapped", "bGluZQpicmVhaw==")
                        """
                    } else {
                        ""
                    }
                val wrapperResponse =
                    if (schemaSerde) {
                        """
                        assert_eq!(values("x-enums"), ["b2JqZWN0", "YXJyYXk="]);
                        assert_eq!(values("x-wrapped"), ["e30=", "bGluZQpicmVhaw=="]);
                        """
                    } else {
                        ""
                    }
                val model =
                    """
                    ${'$'}version: "2"
                    namespace test.mediaheaders
                    use aws.protocols#$protocol
                    @$protocol
                    service Headers { version: "1", operations: [Echo] }
                    @http(uri: "/echo", method: "GET")
                    operation Echo {
                        errors: [Failure]
                        input := {
                            @httpHeader("x-items") items: MediaStrings
                            $wrapperHeaders
                        }
                        output := {
                            @httpHeader("x-items") items: MediaStrings
                            $wrapperHeaders
                            @xmlAttribute attr: String
                        }
                    }
                    @error("server") structure Failure {}
                    @mediaType("application/json") string MediaString
                    list MediaStrings { member: MediaString }
                    @mediaType("application/json")
                    @enum([{ name: "OBJECT", value: "object" }, { name: "ARRAY", value: "array" }])
                    string MediaEnum
                    list MediaEnums { member: MediaEnum }
                    @mediaType("application/json") @length(min: 1)
                    string WrappedString
                    list WrappedStrings { member: WrappedString }
                    """.asSmithyModel()
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings =
                            ObjectNode.parse("""{"codegen":{"schemaSerde":$schemaSerde}}""").expectObjectNode(),
                        cargoCommand = "cargo test --quiet",
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ) { context, crate ->
                    crate.testModule {
                        tokioTest("media_list_headers_are_encoded_after_generated_deserialization") {
                            rustTemplate(
                                """
                                let config = crate::service::HeadersConfig::builder().build();
                                let service = crate::service::Headers::builder(config)
                                    .echo(|input: crate::input::EchoInput| async move {
                                        let items = input.items.as_ref().unwrap();
                                        assert_eq!(items[0], "{}");
                                        $wrapperAssertions
                                        #{Ok}::<_, crate::error::EchoError>(crate::output::EchoOutput::builder()
                                            .items(input.items)
                                            $wrapperOutput
                                            .attr(#{Some}(#{String}::from("value")))
                                            .build())
                                    })
                                    .build_unchecked();
                                for encoded in [
                                    vec!["e30="],
                                    vec!["e30=", "WzEsMl0=", "YSxi", "YSJi", "bGluZQpicmVhaw==", "c25vd21hbiDimIM="],
                                ] {
                                    for combined in [false, true] {
                                        let mut request = #{Http}::Request::builder().method("GET").uri("/echo")
                                            .header("content-type", "$contentType")
                                            $wrapperRequest;
                                        if combined {
                                            request = request.header("x-items", encoded.join(", "));
                                        } else {
                                            for item in &encoded { request = request.header("x-items", *item); }
                                        }
                                        let response = #{Tower}::ServiceExt::oneshot(service.clone(),
                                            request.body(#{Server}::body::to_boxed_sync("")).unwrap()).await.unwrap();
                                        if response.status() != 200 {
                                            let status = response.status();
                                            let bytes = #{Server}::body::collect_body_limited(response.into_body(), 1024 * 1024)
                                                .await.unwrap();
                                            panic!("{status}: {}", #{String}::from_utf8_lossy(&bytes));
                                        }
                                        assert_eq!(response.status(), 200);
                                        let values = |name| response.headers().get_all(name).iter()
                                            .map(|value| value.to_str().unwrap().to_owned()).collect::<#{Vec}<_>>();
                                        assert_eq!(values("x-items"), encoded);
                                        $wrapperResponse
                                    }
                                }
                                """,
                                *preludeScope,
                                "Http" to RuntimeType.http(context.runtimeConfig),
                                "Server" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType(),
                                "Tower" to ServerCargoDependency.tower(context).toType(),
                            )
                        }
                    }
                }
            }
        }
    }
}

internal class ServerSchemaSensitiveHeadersTest {
    @Test
    fun `header schemas preserve sensitivity inherited from targets`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.sensitiveheaders
            use aws.protocols#restJson1
            @restJson1
            service Secrets { version: "1", operations: [Read, ReadValues, ReadKeys] }
            @http(uri: "/", method: "GET")
            operation Read { output: Headers }
            @http(uri: "/values", method: "GET")
            operation ReadValues { output: ValueHeaders }
            @http(uri: "/keys", method: "GET")
            operation ReadKeys { output: KeyHeaders }
            structure Headers {
                @httpHeader("x-scalar") scalar: Secret
                @httpHeader("x-list") list: SecretList
                @httpHeader("x-elements") elements: SecretsList
                @httpPrefixHeaders("x-map-") map: SecretMap
                @httpHeader("x-public") public: String
            }
            structure ValueHeaders { @httpPrefixHeaders("x-values-") values: SecretValues }
            structure KeyHeaders { @httpPrefixHeaders("x-keys-") keys: SecretKeys }
            @sensitive string Secret
            @sensitive list SecretList { member: String }
            list SecretsList { member: Secret }
            @sensitive map SecretMap { key: String, value: String }
            map SecretValues { key: String, value: Secret }
            map SecretKeys { key: Secret, value: String }
            """.asSmithyModel()
        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = ObjectNode.parse("""{"codegen":{"schemaSerde":true}}""").expectObjectNode(),
                cargoCommand = "cargo test --quiet",
            ),
            testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
        ) { _, crate ->
            crate.testModule {
                unitTest("inherited_sensitive_header_schemas") {
                    rustTemplate(
                        """
                        let schema = crate::output::ReadOutput::SCHEMA;
                        for name in ["scalar", "list", "map"] {
                            assert!(schema.member_schema(name).unwrap().sensitive().is_some(), "{name}");
                        }
                        assert!(schema.member_schema("elements").unwrap().member().unwrap().sensitive().is_some());
                        let values = crate::output::ReadValuesOutput::SCHEMA.member_schema("values").unwrap();
                        assert!(values.member().unwrap().sensitive().is_some());
                        let keys = crate::output::ReadKeysOutput::SCHEMA.member_schema("keys").unwrap();
                        assert!(keys.key().unwrap().sensitive().is_some());
                        assert!(schema.member_schema("public").unwrap().sensitive().is_none());
                        """,
                        *preludeScope,
                    )
                }
            }
        }
    }
}

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
                            "Tower" to ServerCargoDependency.tower(context).toType(),
                        )
                    }
                }
            }
        }
    }
}
