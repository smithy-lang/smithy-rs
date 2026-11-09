/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

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

/**
 * Generates a schema-serde server with media-typed list headers, including enum and wrapper
 * values. It sends encoded header values to the server, checks the decoded handler inputs, then
 * checks the base64-encoded response headers. This verifies that member-level media types survive
 * list serialization and deserialization instead of being replaced by plain string handling.
 */
internal class ServerSchemaMediaTypeHeadersTest {
    @Test
    fun `media typed list headers round trip like legacy generated services`() {
        for (protocol in listOf("restJson1", "restXml")) {
            val contentType =
                if (protocol == "restJson1") {
                    "application/json"
                } else {
                    "application/xml"
                }
            for (schemaSerde in listOf(false, true)) {
                // Generated serde media-type formatting passes enum/newtype wrappers directly to
                // base64::encode, which cannot compile. Compare plain strings against
                // generated serde and exercise those wrappers in the schema path separately.
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
                            ServerAdditionalSettings.builder().schemaSerde(schemaSerde).toObjectNode(),
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
                                            let bytes = #{BodyUtil}::BodyExt::collect(#{BodyUtil}::Limited::new(response.into_body(), 1024 * 1024))
                                                .await.unwrap().to_bytes();
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
}
