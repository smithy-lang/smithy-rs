/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldContain
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.tokioTest
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import kotlin.io.path.readText

internal class ServerSchemaDecoratorTest {
    private val model =
        """
        ${'$'}version: "2"
        namespace com.aws.example.schema

        use aws.protocols#restJson1

        @restJson1
        service SchemaService {
            version: "2024-08-29"
            operations: [Echo, Ping]
        }

        @http(uri: "/echo/{name}", method: "POST", code: 201)
        operation Echo {
            input := {
                @required
                @httpLabel
                name: String
                nested: Nested
                kind: Kind
                choice: Choice
            }
            output := {
                nested: Nested
            }
            errors: [BadThing]
        }

        @http(uri: "/ping", method: "GET")
        operation Ping {}

        structure Nested {
            message: String
            tags: StringList
        }

        list StringList {
            member: String
        }

        enum Kind {
            A
            B
        }

        union Choice {
            text: String
            nested: Nested
        }

        @error("client")
        @httpError(400)
        structure BadThing {
            message: String
        }
        """.asSmithyModel()

    private val schemaSerdeParams =
        IntegrationTestParams(
            additionalSettings =
                ObjectNode.builder()
                    .withMember(
                        "codegen",
                        ObjectNode.builder().withMember(ServerCodegenConfig.SCHEMA_SERDE_CONFIG_KEY, true).build(),
                    )
                    .build(),
        )

    @Test
    fun `descriptors expose the service, its operations and their shapes`() {
        val servers =
            serverIntegrationTest(model, schemaSerdeParams) { context, rustCrate ->
                rustCrate.testModule {
                    unitTest("erased_values_report_their_own_schema") {
                        rustTemplate(
                            """
                            fn assert_schema(value: &dyn #{SerializableStruct}, schema: &#{Schema}<'_>) {
                                assert!(::std::ptr::eq(value.schema(), schema));
                            }
                            let nested = crate::model::Nested { message: None, tags: None };
                            assert_schema(&nested, crate::model::Nested::SCHEMA);
                            let choice = crate::model::Choice::Nested(nested);
                            assert_schema(&choice, crate::model::Choice::SCHEMA);
                            let error = crate::error::BadThing { message: None };
                            assert_schema(&error, crate::error::BadThing::SCHEMA);
                            """,
                            "SerializableStruct" to RuntimeType.smithySchema(context.runtimeConfig).resolve("serde::SerializableStruct"),
                            "Schema" to RuntimeType.smithySchema(context.runtimeConfig).resolve("Schema"),
                        )
                    }
                    // The descriptors belong to the schema router, which only HTTP 1.x services use.
                    if (context.runtimeConfig.httpVersion == HttpVersion.Http1x) {
                        unitTest("operation_error_reports_the_active_variant_schema") {
                            rustTemplate(
                                """
                                let error = crate::error::EchoError::BadThing(crate::error::BadThing { message: None });
                                let erased: &dyn #{SerializableStruct} = &error;
                                assert!(::std::ptr::eq(erased.schema(), crate::error::BadThing::SCHEMA));
                                let http_error: &dyn #{HttpModeledError} = &error;
                                assert!(::std::ptr::eq(http_error.schema(), erased.schema()));
                                """,
                                "SerializableStruct" to RuntimeType.smithySchema(context.runtimeConfig).resolve("serde::SerializableStruct"),
                                "HttpModeledError" to ServerCargoDependency.smithyHttpServer(context.runtimeConfig).toType().resolve("schema::HttpModeledError"),
                            )
                        }
                        unitTest("operation_descriptor_reports_the_modeled_http_binding") {
                            rust(
                                """
                        let echo = &crate::schema::operations::ECHO;
                        assert_eq!(echo.shape_id().as_str(), "com.aws.example.schema##Echo");
                        let http = echo.http().expect("`@http` recorded on the operation input");
                        assert_eq!(http.method(), "POST");
                        assert_eq!(http.uri(), "/echo/{name}");
                        assert_eq!(http.code(), 201);
                        assert!(std::ptr::eq(echo.input(), crate::input::EchoInput::SCHEMA));
                        assert!(std::ptr::eq(echo.output(), crate::output::EchoOutput::SCHEMA));
                        // The modeled error plus the `ValidationException` the server attaches for the constrained input.
                        assert_eq!(echo.errors().len(), 2);
                        assert!(std::ptr::eq(echo.errors()[0], crate::error::BadThing::SCHEMA));
                        assert!(std::ptr::eq(echo.errors()[1], crate::error::ValidationException::SCHEMA));
                        """,
                            )
                        }
                        unitTest("service_descriptor_lists_protocols_and_operations") {
                            rust(
                                """
                        let service = &crate::schema::service::SCHEMA_SERVICE;
                        assert_eq!(service.shape_id().as_str(), "com.aws.example.schema##SchemaService");
                        assert_eq!(service.version(), Some("2024-08-29"));
                        let protocols: Vec<_> = service.protocols().iter().map(|id| id.as_str()).collect();
                        assert_eq!(protocols, ["aws.protocols##restJson1"]);
                        assert_eq!(service.operations().len(), 2);
                        let ping = service
                            .operation(&::aws_smithy_schema::shape_id!("com.aws.example.schema", "Ping"))
                            .expect("bound operation");
                        assert_eq!(ping.http().map(|http| http.uri()), Some("/ping"));
                        assert!(ping.errors().is_empty());
                        """,
                            )
                        }
                    }
                    unitTest("shape_schemas_sit_next_to_their_types") {
                        rustTemplate(
                            """
                        use #{ShapeType};
                        // Operation schemas retain their modeled shape IDs.
                        assert_eq!(crate::input::EchoInput::SCHEMA.shape_id().as_str(), "com.aws.example.schema##EchoInput");
                        assert_eq!(crate::model::Nested::SCHEMA.shape_type(), ShapeType::Structure);
                        assert_eq!(crate::model::Nested::SCHEMA.members().len(), 2);
                        assert_eq!(crate::model::Choice::SCHEMA.shape_type(), ShapeType::Union);
                        assert_eq!(crate::error::BadThing::SCHEMA.shape_type(), ShapeType::Structure);
                        assert!(crate::error::BadThing::SCHEMA.traits().unwrap().contains_fqn("smithy.api##error"));
                        """,
                            *preludeScope,
                            "ShapeType" to RuntimeType.smithySchema(context.runtimeConfig).resolve("ShapeType"),
                        )
                    }
                }
            }
        servers.forEach { server ->
            val src = server.path.resolve("src")
            src.resolve("protocol_serde").toFile().exists() shouldBe (server.httpVersion == HttpTestVersion.HTTP_0_X)
            src.resolve("schema").toFile().exists() shouldBe (server.httpVersion == HttpTestVersion.HTTP_1_X)
            if (server.httpVersion == HttpTestVersion.HTTP_1_X) {
                val service = src.resolve("service.rs").readText()
                service shouldContain "from_operation_handler_bindings_with_options"
                service shouldNotContain "request_specs"
                service shouldNotContain "RestRouter"
                service shouldNotContain "SchemaRoute::new"
                src.toFile().walkTopDown().filter { it.isFile }.forEach {
                    it.readText() shouldNotContain "protocol_serde"
                }
            }
        }
    }

    @Test
    fun `error schemas preserve explicit httpError status codes`() {
        serverIntegrationTest(model, schemaSerdeParams) { _, rustCrate ->
            rustCrate.testModule {
                unitTest(
                    "http_error_status",
                    """
                    let http = crate::error::BadThing::SCHEMA.http().expect("should preserve @httpError");
                    assert_eq!(http.code(), 400);
                    assert_eq!(http.method(), "");
                    assert_eq!(http.uri(), "");
                    """,
                )
            }
        }
    }

    @Test
    fun `nothing is generated while the setting is off`() {
        val servers = serverIntegrationTest(model, testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X))
        servers.forEach { server ->
            val src = server.path.resolve("src")
            src.resolve("schema").toFile().exists() shouldBe false
            src.resolve("protocol_serde").toFile().exists() shouldBe true
            src.resolve("input.rs").readText() shouldNotContain "SCHEMA"
        }
    }
}

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
                                let bytes = #{Server}::body::collect_body_limited(response.into_body(), 1024 * 1024)
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
