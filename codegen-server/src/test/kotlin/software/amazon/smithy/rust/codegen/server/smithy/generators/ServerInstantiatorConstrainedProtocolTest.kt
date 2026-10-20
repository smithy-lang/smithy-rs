/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */
package software.amazon.smithy.rust.codegen.server.smithy.generators

import io.kotest.assertions.throwables.shouldThrow
import io.kotest.matchers.string.shouldContain
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ValueSource
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testModule
import software.amazon.smithy.rust.codegen.core.testutil.unitTest
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestType
import software.amazon.smithy.rust.codegen.server.smithy.testutil.HttpTestVersion
import software.amazon.smithy.rust.codegen.server.smithy.testutil.MultiVersionTestFailure
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest

/** Native tests must construct valid typed values before assessing service behavior. */
internal class ServerInstantiatorConstrainedProtocolTest {
    @ParameterizedTest
    @ValueSource(booleans = [true, false])
    fun `native fixtures instantiate constrained values in both public modes`(publicConstrainedTypes: Boolean) {
        val model =
            """
            ${'$'}version: "2"
            namespace test.constrainedfixtures
            use aws.protocols#restJson1
            use smithy.test#httpRequestTests
            use smithy.test#httpResponseTests
            @restJson1
            service Fixtures { version: "1", operations: [Payload, DefaultPayload, OptionalDocument, ModeledDefaults] }
            @http(method: "POST", uri: "/payload")
            @httpRequestTests([{
                id: "PopulatedConstrainedRequest", protocol: "aws.protocols#restJson1",
                method: "POST", uri: "/payload", headers: { "content-type": "application/json" },
                body: "{\"title\":\"fixture\",\"number\":-7,\"bytes\":\"YmxvYg==\",\"tags\":[\"first\",\"second\"],\"entries\":{\"first\":3,\"second\":7},\"choice\":{\"text\":\"union\"},\"color\":\"red\"}",
                bodyMediaType: "application/json",
                params: { body: { title: "fixture", number: -7, bytes: "blob",
                    tags: ["first", "second"], entries: { first: 3, second: 7 },
                    choice: { text: "union" }, color: "red" } }, appliesTo: "server"
            }])
            @httpResponseTests([{
                id: "PopulatedConstrainedResponse", protocol: "aws.protocols#restJson1",
                code: 200, headers: { "content-type": "application/json" },
                body: "{\"title\":\"fixture\",\"number\":-7,\"bytes\":\"YmxvYg==\",\"tags\":[\"first\",\"second\"],\"entries\":{\"first\":3,\"second\":7},\"choice\":{\"text\":\"union\"},\"color\":\"red\"}",
                bodyMediaType: "application/json",
                params: { body: { title: "fixture", number: -7, bytes: "blob",
                    tags: ["first", "second"], entries: { first: 3, second: 7 },
                    choice: { text: "union" }, color: "red" } }, appliesTo: "server"
            }])
            operation Payload {
                input := { @required @httpPayload body: Record }
                output := { @required @httpPayload body: Record }
            }
            @http(method: "POST", uri: "/default-payload")
            @httpRequestTests([{
                id: "RequiredPayloadDefaultRequest", protocol: "aws.protocols#restJson1",
                method: "POST", uri: "/default-payload", headers: { "content-type": "application/json" },
                body: "{}", bodyMediaType: "application/json", params: {}, appliesTo: "server"
            }])
            operation DefaultPayload {
                input := {}
                output := { @required @httpPayload body: Record }
            }
            @http(method: "POST", uri: "/document")
            @httpResponseTests([
                {
                    id: "OmittedOptionalDocumentResponse", protocol: "aws.protocols#restJson1",
                    code: 200, headers: { "content-type": "application/json" },
                    body: "{}", bodyMediaType: "application/json", params: {}, appliesTo: "server"
                },
                {
                    id: "ExplicitNullDocumentResponse", protocol: "aws.protocols#restJson1",
                    code: 200, headers: { "content-type": "application/json" },
                    body: "{\"doc\":null}", bodyMediaType: "application/json",
                    params: { doc: null }, appliesTo: "server"
                }
            ])
            operation OptionalDocument { input := { doc: Document }, output := { doc: Document } }
            @http(method: "POST", uri: "/modeled-defaults")
            @httpResponseTests([
                {
                    id: "GeneratedBuilderModeledDefaults", protocol: "aws.protocols#restJson1",
                    code: 200, headers: { "content-type": "application/json" },
                    body: "{\"title\":\"builder\",\"number\":17,\"color\":\"red\"}",
                    bodyMediaType: "application/json", params: {}, appliesTo: "server"
                },
                {
                    id: "GeneratedBuilderPreservesExplicitValue", protocol: "aws.protocols#restJson1",
                    code: 200, headers: { "content-type": "application/json" },
                    body: "{\"title\":\"explicit\",\"number\":17,\"color\":\"red\"}",
                    bodyMediaType: "application/json", params: { title: "explicit" }, appliesTo: "server"
                }
            ])
            operation ModeledDefaults {
                input := {}
                output := {
                    @default("builder") title: String
                    @default(17) number: Integer
                    @default("red") color: DefaultColor
                }
            }
            structure Record {
                title: Text, number: Count, bytes: Content, tags: BoundedList,
                entries: BoundedMap, choice: Choice, color: LegacyEnum
            }
            @length(min: 1, max: 20) string Text
            @range(min: -10, max: 10) integer Count
            @length(min: 1) blob Content
            @length(min: 1, max: 4) list BoundedList { member: Text }
            @length(min: 1, max: 4) map BoundedMap { key: Text, value: Count }
            union Choice { text: Text, items: BoundedList }
            @enum([{ value: "red", name: "RED" }, { value: "blue", name: "BLUE" }])
            string LegacyEnum
            enum DefaultColor {
                RED = "red"
                BLUE = "blue"
            }
            """.asSmithyModel()
        for (schemaSerde in listOf(false, true)) {
            val generate = {
                serverIntegrationTest(
                    model,
                    IntegrationTestParams(
                        additionalSettings =
                            ObjectNode.parse(
                                """{"codegen":{"schemaSerde":$schemaSerde,"publicConstrainedTypes":$publicConstrainedTypes}}""",
                            ).expectObjectNode(),
                        cargoCommand = "cargo test --quiet --lib --tests",
                    ),
                    testCoverage = HttpTestType.Only(HttpTestVersion.HTTP_1_X),
                ) { context, crate ->
                    val operation = context.model.lookup<OperationShape>("test.constrainedfixtures#Payload")
                    val documentOperation = context.model.lookup<OperationShape>("test.constrainedfixtures#OptionalDocument")
                    crate.testModule {
                        unitTest("required_payload_default_is_emitted_once") {
                            withBlock("let output = ", ";") {
                                ServerInstantiator(context).render(this, operation.outputShape(context.model), Node.objectNode())
                            }
                            rust("assert!(output.body.title.is_none());")
                            rust("assert!(output.body.entries.is_none());")
                        }
                        unitTest("expected_input_distinguishes_omitted_document_and_explicit_null") {
                            withBlock("let omitted = ", ";") {
                                ServerInstantiator(context).render(this, documentOperation.inputShape(context.model), Node.objectNode())
                            }
                            withBlock("let explicit = ", ";") {
                                ServerInstantiator(context).render(
                                    this,
                                    documentOperation.inputShape(context.model),
                                    Node.parse("""{"doc":null}"""),
                                )
                            }
                            rust("assert!(omitted.doc.is_none());")
                            rust("assert_eq!(explicit.doc, Some(::aws_smithy_types::Document::Null));")
                        }
                    }
                }
            }
            if (schemaSerde) {
                generate()
            } else {
                // This exact model fails on fresh mainline at 9267f0e25. Keep the
                // legacy instantiator unchanged and assert its observed limitation.
                val error = shouldThrow<MultiVersionTestFailure> { generate() }
                error.message shouldContain "E0062"
                error.message shouldContain "field `body` specified more than once"
                if (publicConstrainedTypes) error.message shouldContain "E0308"
            }
        }
    }
}
