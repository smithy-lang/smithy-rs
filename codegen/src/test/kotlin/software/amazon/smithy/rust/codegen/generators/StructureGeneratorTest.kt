/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.generators

import io.kotest.matchers.string.shouldContainInOrder
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.Custom
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.raw
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.testutil.unitTest
import software.amazon.smithy.rust.codegen.util.lookup

class StructureGeneratorTest {
    companion object {
        val model = """
        namespace com.test
        @documentation("this documents the shape")
        structure MyStruct {
           foo: String,
           @documentation("This *is* documentation about the member.")
           bar: PrimitiveInteger,
           baz: Integer,
           ts: Timestamp,
           inner: Inner,
           byteValue: Byte
        }

        // Intentionally empty
        structure Inner {
        }

        @error("server")
        @retryable
        structure MyError {
            message: String
        }
        """.asSmithyModel()
        val struct = model.expectShape(ShapeId.from("com.test#MyStruct"), StructureShape::class.java)
        val inner = model.expectShape(ShapeId.from("com.test#Inner"), StructureShape::class.java)
        val error = model.expectShape(ShapeId.from("com.test#MyError"), StructureShape::class.java)
    }

    @Test
    fun `generate basic structures`() {
        val provider = testSymbolProvider(model)
        val project = TestWorkspace.testProject(provider)
        project.useShapeWriter(inner) { writer ->
            StructureGenerator(model, provider, writer, inner).render()
            StructureGenerator(model, provider, writer, struct).render()
            writer.unitTest(
                """
                let s: Option<MyStruct> = None;
                s.map(|i|println!("{:?}, {:?}", i.ts, i.byte_value));
                """
            )
            writer.toString().shouldContainInOrder(
                "this documents the shape", "#[non_exhaustive]", "pub", "struct MyStruct"
            )
        }
        project.compileAndTest()
    }

    @Test
    fun `generate structures with public fields`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.root()
        writer.withModule("model") {
            val innerGenerator = StructureGenerator(model, provider, this, inner)
            innerGenerator.render()
        }
        writer.withModule("structs") {
            val generator = StructureGenerator(model, provider, this, struct)
            generator.render()
        }
        // By putting the test in another module, it can't access the struct
        // fields if they are private
        writer.withModule("inline") {
            raw("#[test]")
            rustBlock("fn test_public_fields()") {
                write(
                    """
                    let s: Option<crate::structs::MyStruct> = None;
                    s.map(|i|println!("{:?}, {:?}", i.ts, i.byte_value));
                """
                )
            }
        }
        writer.compileAndTest()
    }

    @Test
    fun `generate error structures`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("error")
        val generator = StructureGenerator(model, provider, writer, error)
        generator.render()
        writer.compileAndTest(
            """
            let err = MyError { message: None };
            assert_eq!(err.error_kind(), smithy_types::retry::ErrorKind::ServerError);
        """
        )
    }

    @Test
    fun `attach docs to everything`() {
        val model = """
        namespace com.test
        @documentation("inner doc")
        structure Inner { }

        @documentation("shape doc")
        structure MyStruct {
           @documentation("member doc")
           member: String,

           @documentation("specific docs")
           nested: Inner,

           nested2: Inner
        }""".asSmithyModel()
        val provider = testSymbolProvider(model)
        val writer = RustWriter.root()
        writer.docs("module docs")
        writer
            .withModule(
                "model",
                // By attaching this lint, any missing documentation becomes a compiler erorr
                RustMetadata(additionalAttributes = listOf(Custom("deny(missing_docs)")), public = true)
            ) {
                StructureGenerator(model, provider, this, model.lookup("com.test#Inner")).render()
                StructureGenerator(model, provider, this, model.lookup("com.test#MyStruct")).render()
            }

        writer.compileAndTest()
    }
}
