/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.EnumDefinition
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.MaybeRenamed
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.core.smithy.setDefault
import software.amazon.smithy.rust.codegen.core.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider

internal class BuilderGeneratorTest {
    private val model = StructureGeneratorTest.model
    private val inner = StructureGeneratorTest.inner
    private val struct = StructureGeneratorTest.struct
    private val credentials = StructureGeneratorTest.credentials
    private val secretStructure = StructureGeneratorTest.secretStructure

    @Test
    fun `generate builders`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        writer.rust("##![allow(deprecated)]")
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
            """,
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
            }
        val writer = RustWriter.forModule("model")
        writer.rust("##![allow(deprecated)]")
        val innerGenerator = StructureGenerator(
            StructureGeneratorTest.model, provider, writer,
            StructureGeneratorTest.inner,
        )
        val generator = StructureGenerator(
            StructureGeneratorTest.model, provider, writer,
            StructureGeneratorTest.struct,
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
            """,
        )
    }

    @Test
    fun `builder for a struct with sensitive fields should implement the debug trait as such`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val credsGenerator = StructureGenerator(model, provider, writer, credentials)
        val builderGenerator = BuilderGenerator(model, provider, credentials)
        credsGenerator.render()
        builderGenerator.render(writer)
        writer.implBlock(credentials, provider) {
            builderGenerator.renderConvenienceMethod(this)
        }
        writer.compileAndTest(
            """
            use super::*;
            let builder = Credentials::builder()
                .username("admin")
                .password("pswd")
                .secret_key("12345");
                 assert_eq!(format!("{:?}", builder), "Builder { username: Some(\"admin\"), password: \"*** Sensitive Data Redacted ***\", secret_key: \"*** Sensitive Data Redacted ***\" }");
            """,
        )
    }

    @Test
    fun `builder for a sensitive struct should implement the debug trait as such`() {
        val provider = testSymbolProvider(model)
        val writer = RustWriter.forModule("model")
        val structGenerator = StructureGenerator(model, provider, writer, secretStructure)
        val builderGenerator = BuilderGenerator(model, provider, secretStructure)
        structGenerator.render()
        builderGenerator.render(writer)
        writer.implBlock(secretStructure, provider) {
            builderGenerator.renderConvenienceMethod(this)
        }
        writer.compileAndTest(
            """
            use super::*;
            let builder = SecretStructure::builder()
                .secret_field("secret");
            assert_eq!(format!("{:?}", builder), "Builder { secret_field: \"*** Sensitive Data Redacted ***\" }");
            """,
        )
    }
}
