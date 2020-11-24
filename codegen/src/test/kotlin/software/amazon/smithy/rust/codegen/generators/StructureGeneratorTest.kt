/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.lang.Custom
import software.amazon.smithy.rust.codegen.lang.RustMetadata
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.lang.docs
import software.amazon.smithy.rust.codegen.lang.rustBlock
import software.amazon.smithy.rust.codegen.smithy.canUseDefault
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider

class StructureGeneratorTest {
    private val model = """
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
        structure MyError {
            message: String
        }
        """.asSmithyModel()
    private val struct = model.expectShape(ShapeId.from("com.test#MyStruct"), StructureShape::class.java)
    private val inner = model.expectShape(ShapeId.from("com.test#Inner"), StructureShape::class.java)
    private val error = model.expectShape(ShapeId.from("com.test#MyError"), StructureShape::class.java)

    @Test
    fun `generate basic structures`() {
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val innerGenerator = StructureGenerator(model, provider, writer, inner)
        val generator = StructureGenerator(model, provider, writer, struct)
        generator.render()
        innerGenerator.render()
        writer.compileAndTest(
            """
            let s: Option<MyStruct> = None;
            s.map(|i|println!("{:?}, {:?}", i.ts, i.byte_value));
            """.trimIndent()
        )
    }

    @Test
    fun `generate structures with public fields`() {
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val innerGenerator = StructureGenerator(model, provider, writer, inner, renderBuilder = false)
        innerGenerator.render()
        writer.withModule("structs") {
            val generator = StructureGenerator(model, provider, this, struct, renderBuilder = false)
            generator.render()
        }
        // By putting the test in another module, it can't access the struct
        // fields if they are private
        writer.withModule("inline") {
            write("#[test]")
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
    fun `generate builders`() {
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val innerGenerator = StructureGenerator(model, provider, writer, inner)
        val generator = StructureGenerator(model, provider, writer, struct)
        generator.render()
        innerGenerator.render()
        writer.compileAndTest(
            """
            let my_struct = MyStruct::builder().byte_value(4).foo("hello!").build();
            assert_eq!(my_struct.foo.unwrap(), "hello!");
            assert_eq!(my_struct.bar, 0);
        """
        )
    }

    @Test
    fun `generate fallible builders`() {
        val baseProvider: SymbolProvider = testSymbolProvider(model)
        val provider =
            object : SymbolProvider {
                override fun toSymbol(shape: Shape?): Symbol {
                    return baseProvider.toSymbol(shape).toBuilder().canUseDefault(false).build()
                }

                override fun toMemberName(shape: MemberShape?): String {
                    return baseProvider.toMemberName(shape)
                }
            }
        val writer = RustWriter.forModule("model")
        val innerGenerator = StructureGenerator(model, provider, writer, inner)
        val generator = StructureGenerator(model, provider, writer, struct)
        generator.render()
        innerGenerator.render()
        writer.compileAndTest(
            """
            let my_struct = MyStruct::builder().byte_value(4).foo("hello!").bar(0).build().expect("required field was not provided");
            assert_eq!(my_struct.foo.unwrap(), "hello!");
            assert_eq!(my_struct.bar, 0);
        """
        )
    }

    @Test
    fun `generate error structures`() {
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule("error")
        val generator = StructureGenerator(model, provider, writer, error)
        generator.render()
        writer.compileAndTest()
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
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter.forModule(null)
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
