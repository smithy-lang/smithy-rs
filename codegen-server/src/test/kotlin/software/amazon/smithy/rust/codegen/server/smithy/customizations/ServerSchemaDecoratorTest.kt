/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customizations

import io.kotest.matchers.shouldBe
import io.kotest.matchers.string.shouldNotContain
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
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
        serverIntegrationTest(model, schemaSerdeParams) { _, rustCrate ->
            rustCrate.testModule {
                unitTest("operation_descriptor_reports_the_modeled_http_binding") {
                    rust(
                        """
                        let echo = &crate::schema::operations::ECHO;
                        assert_eq!(echo.shape_id().as_str(), "com.aws.example.schema##Echo");
                        let http = echo.schema().http().expect("`@http` on the operation shape");
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
                        assert_eq!(ping.schema().http().map(|http| http.uri()), Some("/ping"));
                        assert!(ping.errors().is_empty());
                        """,
                    )
                }
                unitTest("shape_schemas_sit_next_to_their_types") {
                    rust(
                        """
                        use ::aws_smithy_schema::ShapeType;
                        // Operation inputs and outputs are synthetic shapes, so they carry the synthetic namespace.
                        assert_eq!(crate::input::EchoInput::SCHEMA.shape_id().as_str(), "com.aws.example.schema.synthetic##EchoInput");
                        assert_eq!(crate::model::Nested::SCHEMA.shape_type(), ShapeType::Structure);
                        assert_eq!(crate::model::Nested::SCHEMA.members().len(), 2);
                        assert_eq!(crate::model::Choice::SCHEMA.shape_type(), ShapeType::Union);
                        assert_eq!(crate::error::BadThing::SCHEMA.shape_type(), ShapeType::Structure);
                        """,
                    )
                }
            }
        }
    }

    @Test
    fun `nothing is generated while the setting is off`() {
        val servers = serverIntegrationTest(model, testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X))
        servers.forEach { server ->
            val src = server.path.resolve("src")
            src.resolve("schema").toFile().exists() shouldBe false
            src.resolve("input.rs").readText() shouldNotContain "SCHEMA"
        }
    }
}
