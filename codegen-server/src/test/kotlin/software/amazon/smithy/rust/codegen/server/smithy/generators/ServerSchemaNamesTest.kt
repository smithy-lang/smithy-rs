/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext

class ServerSchemaNamesTest {
    @Test
    fun `shape collisions get successive suffixes within their Rust module`() {
        val model =
            """
            namespace test
            structure A {}
            structure B {}
            structure C {}
            structure D {}
            structure Unrelated {}
            """.asSmithyModel()
        val provider =
            object : WrappingSymbolProvider(serverTestCodegenContext(model).symbolProvider) {
                override fun toSymbol(shape: Shape): Symbol {
                    val symbol = super.toSymbol(shape)
                    return when (shape.id.name) {
                        "A" -> symbol.toBuilder().name("Widget").build()
                        "B" -> symbol.toBuilder().name("WIDGET").build()
                        "C" -> symbol.toBuilder().name("widget").build()
                        "D" -> symbol.toBuilder().name("Widget").namespace("crate::other", "::").build()
                        else -> symbol
                    }
                }
            }
        val names = ServerSchemaNames(model, provider, model.structureShapes.filter { it.id.namespace == "test" })

        fun prefix(name: String): String = names.forShape(model.lookup<StructureShape>("test#$name")).prefix

        // Lookup order does not affect allocation.
        assertEquals("SHAPE_WIDGET_2", prefix("C"))
        assertEquals("SHAPE_WIDGET", prefix("A"))
        assertEquals("SHAPE_WIDGET_1", prefix("B"))
        assertEquals("SHAPE_WIDGET", prefix("D"))
        assertEquals("SHAPE_UNRELATED", prefix("Unrelated"))
        assertEquals("SHAPE_WIDGET_2", prefix("C"))
    }

    @Test
    fun `member names avoid nested aggregate statics and preserve raw identifiers`() {
        val model =
            """
            namespace test
            structure Example {
                x: Values
                xKey: String
                xValueMember: String
                l: Items
                lMember: String
                enum: String
            }
            map Values { key: String, value: Items }
            list Items { member: String }
            """.asSmithyModel()
        val context = serverTestCodegenContext(model)
        val shape = model.lookup<StructureShape>("test#Example")
        val names = context.schemaNames

        fun member(name: String): String = names.getMemberName(shape.getMember(name).get().id)

        assertEquals("SHAPE_EXAMPLE_MEMBER_X", member("x"))
        assertEquals("SHAPE_EXAMPLE_MEMBER_X_KEY_1", member("xKey"))
        assertEquals("SHAPE_EXAMPLE_MEMBER_X_VALUE_MEMBER_1", member("xValueMember"))
        assertEquals("SHAPE_EXAMPLE_MEMBER_L", member("l"))
        assertEquals("SHAPE_EXAMPLE_MEMBER_L_MEMBER_1", member("lMember"))
        assertEquals("SHAPE_EXAMPLE_MEMBER_ENUM", member("enum"))
    }
}
