/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.EnumSource
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.node.ObjectNode
import software.amazon.smithy.rust.codegen.core.testutil.IntegrationTestParams
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverIntegrationTest
import java.io.File

/**
 * Tests for http-1x flag with different protocols.
 * Phase 2: Compilation - verifies that generated code compiles successfully with both HTTP/0 and HTTP/1.
 */
internal class Http1xProtocolCompilationTest {
    private fun buildAdditionalSettings(http1x: Boolean): ObjectNode =
        Node.objectNodeBuilder()
            .withMember(
                "codegen",
                Node.objectNodeBuilder()
                    .withMember("http-1x", http1x)
                    .build(),
            ).build()

    @ParameterizedTest
    @EnumSource(ModelProtocol::class)
    fun `generated code compiles with http-1x disabled for all protocols`(protocol: ModelProtocol) {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(protocol)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        )
    }

    @ParameterizedTest
    @EnumSource(ModelProtocol::class)
    fun `generated code compiles with http-1x enabled for all protocols`(protocol: ModelProtocol) {
        val (model, serviceShapeId) = loadSmithyConstraintsModelForProtocol(protocol)

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                service = serviceShapeId.toString(),
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        )
    }

    @Test
    fun `pokemon service compiles with http-1x disabled`() {
        val model = File("../codegen-core/common-test-models/pokemon.smithy").readText().asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        )
    }

    @Test
    fun `pokemon service compiles with http-1x enabled`() {
        val model = File("../codegen-core/common-test-models/pokemon.smithy").readText().asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        )
    }

    @Test
    fun `simple service with multiple operations compiles with http-1x disabled`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "2024-03-18"
                operations: [GetItem, ListItems, CreateItem, UpdateItem, DeleteItem]
            }

            @http(uri: "/items/{id}", method: "GET")
            operation GetItem {
                input: GetItemInput
                output: GetItemOutput
                errors: [ItemNotFound]
            }

            @http(uri: "/items", method: "GET")
            operation ListItems {
                input: ListItemsInput
                output: ListItemsOutput
            }

            @http(uri: "/items", method: "POST")
            operation CreateItem {
                input: CreateItemInput
                output: CreateItemOutput
            }

            @http(uri: "/items/{id}", method: "PUT")
            operation UpdateItem {
                input: UpdateItemInput
                output: UpdateItemOutput
                errors: [ItemNotFound]
            }

            @http(uri: "/items/{id}", method: "DELETE")
            operation DeleteItem {
                input: DeleteItemInput
                output: DeleteItemOutput
                errors: [ItemNotFound]
            }

            structure GetItemInput {
                @httpLabel
                @required
                id: String
            }

            structure GetItemOutput {
                @required
                item: Item
            }

            structure ListItemsInput {
                @httpQuery("limit")
                limit: Integer

                @httpQuery("nextToken")
                nextToken: String
            }

            structure ListItemsOutput {
                @required
                items: ItemList

                nextToken: String
            }

            structure CreateItemInput {
                @required
                name: String

                description: String
            }

            structure CreateItemOutput {
                @required
                item: Item
            }

            structure UpdateItemInput {
                @httpLabel
                @required
                id: String

                name: String
                description: String
            }

            structure UpdateItemOutput {
                @required
                item: Item
            }

            structure DeleteItemInput {
                @httpLabel
                @required
                id: String
            }

            structure DeleteItemOutput {}

            structure Item {
                @required
                id: String

                @required
                name: String

                description: String
            }

            list ItemList {
                member: Item
            }

            @error("client")
            @httpError(404)
            structure ItemNotFound {
                @required
                message: String
            }
            """.asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = false),
            ),
        )
    }

    @Test
    fun `simple service with multiple operations compiles with http-1x enabled`() {
        val model =
            """
            namespace test

            use aws.protocols#restJson1

            @restJson1
            service TestService {
                version: "2024-03-18"
                operations: [GetItem, ListItems, CreateItem, UpdateItem, DeleteItem]
            }

            @http(uri: "/items/{id}", method: "GET")
            operation GetItem {
                input: GetItemInput
                output: GetItemOutput
                errors: [ItemNotFound]
            }

            @http(uri: "/items", method: "GET")
            operation ListItems {
                input: ListItemsInput
                output: ListItemsOutput
            }

            @http(uri: "/items", method: "POST")
            operation CreateItem {
                input: CreateItemInput
                output: CreateItemOutput
            }

            @http(uri: "/items/{id}", method: "PUT")
            operation UpdateItem {
                input: UpdateItemInput
                output: UpdateItemOutput
                errors: [ItemNotFound]
            }

            @http(uri: "/items/{id}", method: "DELETE")
            operation DeleteItem {
                input: DeleteItemInput
                output: DeleteItemOutput
                errors: [ItemNotFound]
            }

            structure GetItemInput {
                @httpLabel
                @required
                id: String
            }

            structure GetItemOutput {
                @required
                item: Item
            }

            structure ListItemsInput {
                @httpQuery("limit")
                limit: Integer

                @httpQuery("nextToken")
                nextToken: String
            }

            structure ListItemsOutput {
                @required
                items: ItemList

                nextToken: String
            }

            structure CreateItemInput {
                @required
                name: String

                description: String
            }

            structure CreateItemOutput {
                @required
                item: Item
            }

            structure UpdateItemInput {
                @httpLabel
                @required
                id: String

                name: String
                description: String
            }

            structure UpdateItemOutput {
                @required
                item: Item
            }

            structure DeleteItemInput {
                @httpLabel
                @required
                id: String
            }

            structure DeleteItemOutput {}

            structure Item {
                @required
                id: String

                @required
                name: String

                description: String
            }

            list ItemList {
                member: Item
            }

            @error("client")
            @httpError(404)
            structure ItemNotFound {
                @required
                message: String
            }
            """.asSmithyModel()

        serverIntegrationTest(
            model,
            IntegrationTestParams(
                additionalSettings = buildAdditionalSettings(http1x = true),
            ),
        )
    }
}
