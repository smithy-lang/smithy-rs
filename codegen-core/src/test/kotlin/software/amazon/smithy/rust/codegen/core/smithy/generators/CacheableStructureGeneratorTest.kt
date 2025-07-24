/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import kotlin.test.assertTrue

class CacheableStructureGeneratorTest {
    companion object {
        val model =
            Model.assembler()
                .addImport(CacheableStructureGeneratorTest::class.java.getResource("/cacheable-test.smithy"))
                .assemble()
                .unwrap()
    }

    @Test
    fun `test structure generator wraps cacheable members`() {
        val symbolProvider = RustSymbolProvider(SymbolVisitor(model))
        val writer = RustWriter("test")

        // Get the GetUserDataOutput structure which has a cacheable member
        val outputShape = model.expectShape(ShapeId.from("example.cacheable#GetUserDataOutput"), StructureShape::class.java)

        // Create a structure generator for the output shape
        val generator =
            StructureGenerator(
                model,
                symbolProvider,
                writer,
                outputShape,
                emptyList(),
                StructSettings(flattenVecAccessors = false),
            )

        // Generate the structure
        generator.render()

        // Get the generated code
        val generatedCode = writer.toString()

        // Verify that the userData field is wrapped in Cacheable<T>
        assertTrue(
            generatedCode.contains("userData: Cacheable<UserData>"),
            "Generated code should wrap userData in Cacheable<T>",
        )

        // Verify that the requestId field is not wrapped
        assertTrue(
            generatedCode.contains("requestId: String"),
            "Generated code should not wrap requestId",
        )
    }

    @Test
    fun `test structure generator adds cacheable imports`() {
        val symbolProvider = RustSymbolProvider(SymbolVisitor(model))
        val writer = RustWriter("test")

        // Get the GetUserDataOutput structure which has a cacheable member
        val outputShape = model.expectShape(ShapeId.from("example.cacheable#GetUserDataOutput"), StructureShape::class.java)

        // Create a structure generator for the output shape
        val generator =
            StructureGenerator(
                model,
                symbolProvider,
                writer,
                outputShape,
                emptyList(),
                StructSettings(flattenVecAccessors = false),
            )

        // Generate the structure
        generator.render()

        // Get the generated code
        val generatedCode = writer.toString()

        // Verify that the Bytes import is added
        assertTrue(
            generatedCode.contains("use bytes::Bytes;"),
            "Generated code should import bytes::Bytes",
        )
    }
}
