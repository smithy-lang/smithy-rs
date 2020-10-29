/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.generators

import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.DocumentationTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.lang.RustWriter
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.testutil.shouldCompile
import software.amazon.smithy.rust.testutil.testSymbolProvider

class StructureGeneratorTest {
    private val model: Model
    private val struct: StructureShape
    private val error: StructureShape
    init {
        val member1 = MemberShape.builder().id("com.test#MyStruct\$foo").target("smithy.api#String").build()
        val member2 = MemberShape.builder().id("com.test#MyStruct\$bar").target("smithy.api#PrimitiveInteger").addTrait(
            DocumentationTrait("This *is* documentation about the member.")
        ).build()
        val member3 = MemberShape.builder().id("com.test#MyStruct\$baz").target("smithy.api#Integer").build()
        val member4 = MemberShape.builder().id("com.test#MyStruct\$ts").target("smithy.api#Timestamp").build()

        // struct 2 will be of type `Qux` under `MyStruct::quux` member
        val struct2 = StructureShape.builder()
            .id("com.test#Qux")
            .build()
        // structure member shape - note the capitalization of the member name (generated code should use the Kotlin class member name)
        // val member4 = MemberShape.builder().id("com.test#MyStruct\$Quux").target(struct2).build()
        val member5 = MemberShape.builder().id("com.test#MyStruct\$byteValue").target("smithy.api#Byte").build()

        struct = StructureShape.builder()
            .id("com.test#MyStruct")
            .addMember(member1)
            .addMember(member2)
            .addMember(member3)
            .addMember(member4)
            .addMember(member5)
            .addTrait(DocumentationTrait("This *is* documentation about the shape."))
            .build()

        val messageMember = MemberShape.builder().id("com.test#MyError\$message").target("smithy.api#String").build()

        error = StructureShape.builder()
            .id("com.test#MyError")
            .addTrait(ErrorTrait("server"))
            .addMember(messageMember).build()
        model = Model.assembler()
            .addShapes(struct, error, struct2, member1, member2, member3, messageMember)
            .assemble()
            .unwrap()
    }

    @Test
    fun `generate basic structures`() {
        val provider: SymbolProvider = testSymbolProvider(model)
        val writer = RustWriter("model.rs", "model")
        val generator = StructureGenerator(model, provider, writer, struct)
        generator.render()
        writer.shouldCompile("""
            let s: Option<MyStruct> = None;
            s.map(|i|println!("{:?}, {:?}", i.ts, i.byte_value));
        """.trimIndent())
    }

    @Test
    fun `generate error structures`() {
        val provider: SymbolProvider = SymbolVisitor(model, "test")
        val writer = RustWriter("errors.rs", "errors")
        val generator = StructureGenerator(model, provider, writer, error)
        generator.render()
        writer.shouldCompile()
    }
}
