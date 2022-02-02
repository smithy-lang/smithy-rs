/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.generators.StructureGeneratorTest
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.Default
import software.amazon.smithy.rust.codegen.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.setDefault
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider

internal class BuilderGeneratorTest {
    private val model = StructureGeneratorTest.model
    private val inner = StructureGeneratorTest.inner
    private val struct = StructureGeneratorTest.struct

    @Test
    fun `generate builders`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val innerGenerator = StructureGenerator(model, provider, writer, inner)
        val generator = StructureGenerator(model, provider, writer, struct)
        val builderGenerator = BuilderGenerator(model, provider, struct)
        generator.render()
        innerGenerator.render()
        builderGenerator.render(writer)
        writer.implBlock(struct, provider) {
            builderGenerator.renderConvenienceMethod(this)
        }
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
        val baseProvider: RustSymbolProvider = testSymbolProvider(StructureGeneratorTest.model)
        val provider =
            object : RustSymbolProvider {
                override fun config(): SymbolVisitorConfig {
                    return baseProvider.config()
                }

                override fun toEnumVariantName(definition: EnumDefinition): MaybeRenamed? {
                    return baseProvider.toEnumVariantName(definition)
                }

                override fun toSymbol(shape: Shape?): Symbol {
                    return baseProvider.toSymbol(shape).toBuilder().setDefault(Default.NoDefault).build()
                }

                override fun toMemberName(shape: MemberShape?): String {
                    return baseProvider.toMemberName(shape)
                }

                override fun isRequiredTraitHandled(member: MemberShape, useNullableIndex: Boolean): Boolean {
                    return false
                }
            }
        val writer = RustWriter.forModule("model")
        val innerGenerator = StructureGenerator(
            StructureGeneratorTest.model, provider, writer,
            StructureGeneratorTest.inner
        )
        val generator = StructureGenerator(
            StructureGeneratorTest.model, provider, writer,
            StructureGeneratorTest.struct
        )
        generator.render()
        innerGenerator.render()
        val builderGenerator = BuilderGenerator(model, provider, struct)
        builderGenerator.render(writer)
        writer.implBlock(struct, provider) {
            builderGenerator.renderConvenienceMethod(this)
        }
        writer.compileAndTest(
            """
            let my_struct = MyStruct::builder().byte_value(4).foo("hello!").bar(0).build().expect("required field was not provided");
            assert_eq!(my_struct.foo.unwrap(), "hello!");
            assert_eq!(my_struct.bar, 0);
            """
        )
    }
}
