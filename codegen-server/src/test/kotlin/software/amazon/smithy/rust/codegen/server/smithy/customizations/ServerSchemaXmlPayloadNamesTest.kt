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

internal class ServerSchemaXmlPayloadNamesTest {
    @Test
    fun `XML structured payload roots match legacy precedence`() {
        val model =
            """
            ${'$'}version: "2"
            namespace test.xmlpayload
            use aws.protocols#restXml
            @restXml
            service Payloads { version: "1", operations: [Fallback, TargetOnly, MemberOnly, Both, Shared, UnionPayload] }
            @http(uri: "/fallback", method: "GET")
            operation Fallback { output := { @httpPayload body: Target } }
            @http(uri: "/target", method: "GET")
            operation TargetOnly { output := { @httpPayload body: Renamed } }
            @http(uri: "/member", method: "GET")
            operation MemberOnly { output := { @httpPayload @xmlName("WireMember") body: Target } }
            @http(uri: "/both", method: "GET")
            operation Both { output := { @httpPayload @xmlName("WireBoth") body: Renamed } }
            @http(uri: "/shared", method: "GET")
            operation Shared { output := { @httpPayload @xmlName("OtherRoot") body: Renamed } }
            @http(uri: "/union", method: "GET")
            operation UnionPayload { output := { @httpPayload @xmlName("WireUnion") body: Choice } }
            structure Target {
                @xmlName("Child") child: String
                @xmlAttribute attr: String
            }
            @xmlName("RenamedTarget")
            @xmlNamespace(uri: "urn:target", prefix: "t")
            structure Renamed {
                @xmlName("Child") child: String
                @xmlAttribute attr: String
            }
            @xmlName("RenamedUnion")
            @xmlNamespace(uri: "urn:union")
            union Choice { @xmlName("Child") child: String }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
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
                    tokioTest("payload_root_names_and_namespaces") {
                        rustTemplate(
                            """
                            let target = || crate::model::Target::builder()
                                .child(#{Some}(#{String}::from("value"))).attr(#{Some}(#{String}::from("a"))).build();
                            let renamed = || crate::model::Renamed::builder()
                                .child(#{Some}(#{String}::from("value"))).attr(#{Some}(#{String}::from("a"))).build();
                            let config = crate::service::PayloadsConfig::builder().build();
                            let service = crate::service::Payloads::builder(config)
                                .fallback(move |_: crate::input::FallbackInput| async move {
                                    crate::output::FallbackOutput::builder().body(#{Some}(target())).build()
                                })
                                .target_only(move |_: crate::input::TargetOnlyInput| async move {
                                    crate::output::TargetOnlyOutput::builder().body(#{Some}(renamed())).build()
                                })
                                .member_only(move |_: crate::input::MemberOnlyInput| async move {
                                    crate::output::MemberOnlyOutput::builder().body(#{Some}(target())).build()
                                })
                                .both(move |_: crate::input::BothInput| async move {
                                    crate::output::BothOutput::builder().body(#{Some}(renamed())).build()
                                })
                                .shared(move |_: crate::input::SharedInput| async move {
                                    crate::output::SharedOutput::builder().body(#{Some}(renamed())).build()
                                })
                                .union_payload(|_: crate::input::UnionPayloadInput| async {
                                    crate::output::UnionPayloadOutput::builder()
                                        .body(#{Some}(crate::model::Choice::Child(#{String}::from("value")))).build()
                                })
                                .build_unchecked();
                            for (path, expected) in [
                                ("/fallback", r##"<Target attr="a"><Child>value</Child></Target>"##),
                                ("/target", r##"<RenamedTarget xmlns:t="urn:target" attr="a"><Child>value</Child></RenamedTarget>"##),
                                ("/member", r##"<WireMember attr="a"><Child>value</Child></WireMember>"##),
                                ("/both", r##"<WireBoth xmlns:t="urn:target" attr="a"><Child>value</Child></WireBoth>"##),
                                ("/shared", r##"<OtherRoot xmlns:t="urn:target" attr="a"><Child>value</Child></OtherRoot>"##),
                                ("/union", r##"<WireUnion xmlns="urn:union"><Child>value</Child></WireUnion>"##),
                                ("/both", r##"<WireBoth xmlns:t="urn:target" attr="a"><Child>value</Child></WireBoth>"##),
                            ] {
                                let request = #{Http}::Request::builder().method("GET").uri(path)
                                    .body(#{Server}::body::to_boxed_sync("")).unwrap();
                                let response = #{Tower}::ServiceExt::oneshot(service.clone(), request).await.unwrap();
                                assert_eq!(response.status(), 200, "{path}");
                                let length = response.headers().get("content-length").unwrap().to_str().unwrap()
                                    .parse::<usize>().unwrap();
                                let bytes = #{Server}::body::__private::collect_body_limited(response.into_body(), 1024 * 1024)
                                    .await.unwrap();
                                assert_eq!(length, bytes.len());
                                assert_eq!(bytes.as_ref(), expected.as_bytes(), "{path}");
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
