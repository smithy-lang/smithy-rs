/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy

import io.kotest.matchers.collections.shouldContain
import io.kotest.matchers.collections.shouldNotBeEmpty
import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Assertions
import org.junit.jupiter.api.DisplayName
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.codegen.core.SymbolProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.loader.ModelAssembler
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.SparseTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.rustlang.render
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.testutil.testSymbolProvider

class SymbolVisitorTest {
    private fun Symbol.referenceClosure(): List<Symbol> {
        val referencedSymbols = this.references.map { it.symbol }
        return listOf(this) + referencedSymbols.flatMap { it.referenceClosure() }
    }

    @Test
    fun `creates structures`() {
        val memberBuilder = MemberShape.builder().id("foo.bar#MyStruct\$someField").target("smithy.api#String")
        val member = memberBuilder.build()
        val struct =
            StructureShape.builder()
                .id("foo.bar#MyStruct")
                .addMember(member)
                .build()
        val model =
            Model.assembler()
                .addShapes(struct, member)
                .assemble()
                .unwrap()
        val provider: SymbolProvider = testSymbolProvider(model)
        val sym = provider.toSymbol(struct)
        sym.rustType().render(false) shouldBe "MyStruct"
        sym.definitionFile shouldBe "src/test_model.rs"
        sym.namespace shouldBe "crate::test_model"
    }

    @Test
    fun `renames errors`() {
        val memberBuilder = MemberShape.builder().id("foo.bar#TerribleException\$someField").target("smithy.api#String")
        val member = memberBuilder.build()
        val struct =
            StructureShape.builder()
                .id("foo.bar#TerribleException")
                .addMember(member)
                .addTrait(ErrorTrait("server"))
                .build()
        val model =
            Model.assembler()
                .addShapes(struct, member)
                .assemble()
                .unwrap()
        val provider: SymbolProvider = testSymbolProvider(model)
        val sym = provider.toSymbol(struct)
        sym.rustType().render(false) shouldBe "TerribleError"
        sym.definitionFile shouldBe "src/test_error.rs"
    }

    @Test
    fun `creates enums`() {
        val model =
            """
            namespace test

            @enum([
                {
                    value: "Count",
                    name: "COUNT",
                },
                {
                    value: "None",
                    name: "NONE",
                }
            ])
            string StandardUnit
            """.asSmithyModel()
        val shape = model.expectShape(ShapeId.from("test#StandardUnit"))
        val provider: SymbolProvider = testSymbolProvider(model)
        val sym = provider.toSymbol(shape)
        sym.rustType().render(false) shouldBe "StandardUnit"
        sym.definitionFile shouldBe "src/test_model.rs"
        sym.namespace shouldBe "crate::test_model"
    }

    @DisplayName("Creates primitives")
    @ParameterizedTest(name = "{index} ==> ''{0}''")
    @CsvSource(
        "String, true, String",
        "Integer, true, i32",
        "PrimitiveInteger, false, i32",
        "Short, true, i16",
        "PrimitiveShort, false, i16",
        "Long, true, i64",
        "PrimitiveLong, false, i64",
        "Byte, true, i8",
        "PrimitiveByte, false, i8",
        "Float, true, f32",
        "PrimitiveFloat, false, f32",
        "Double, true, f64",
        "PrimitiveDouble, false, f64",
        "Boolean, true, bool",
        "PrimitiveBoolean, false, bool",
    )
    fun `creates primitives`(
        primitiveType: String,
        optional: Boolean,
        rustName: String,
    ) {
        val model =
            """
            namespace foo.bar
            structure MyStruct {
                quux: $primitiveType
            }
            """.asSmithyModel()
        val member = model.expectShape(ShapeId.from("foo.bar#MyStruct\$quux"))
        val provider: SymbolProvider = testSymbolProvider(model)
        val memberSymbol = provider.toSymbol(member)
        // builtins should not have a namespace set
        Assertions.assertEquals("", memberSymbol.namespace)
        Assertions.assertEquals(optional, memberSymbol.isOptional())

        if (!memberSymbol.isOptional()) {
            Assertions.assertEquals(rustName, memberSymbol.rustType().render(false))
        } else {
            Assertions.assertEquals("Option<$rustName>", memberSymbol.rustType().render(false))
        }
    }

    @Test
    fun `creates sets of strings`() {
        val stringShape = StringShape.builder().id("test#Hello").build()
        val set =
            SetShape.builder()
                .id("foo.bar#Records")
                .member(stringShape.id)
                .build()
        val model =
            Model.assembler()
                .addShapes(set, stringShape)
                .assemble()
                .unwrap()

        val provider: SymbolProvider = testSymbolProvider(model)
        val setSymbol = provider.toSymbol(set)
        setSymbol.rustType().render(false) shouldBe "${RustType.HashSet.Type}::<String>"
        setSymbol.referenceClosure().map { it.name } shouldBe listOf(RustType.HashSet.Type, "String")
    }

    @Test
    fun `create vec instead for non-strings`() {
        val struct = StructureShape.builder().id("foo.bar#Record").build()
        val setMember = MemberShape.builder().id("foo.bar#Records\$member").target(struct).build()
        val set =
            SetShape.builder()
                .id("foo.bar#Records")
                .member(setMember)
                .build()
        val model =
            Model.assembler()
                .addShapes(set, setMember, struct)
                .assemble()
                .unwrap()

        val provider: SymbolProvider = testSymbolProvider(model)
        val setSymbol = provider.toSymbol(set)
        setSymbol.rustType().render(false) shouldBe "Vec::<Record>"
        setSymbol.referenceClosure().map { it.name } shouldBe listOf("Vec", "Record")
    }

    @Test
    fun `create sparse collections`() {
        val struct = StructureShape.builder().id("foo.bar#Record").build()
        val setMember = MemberShape.builder().id("foo.bar#Records\$member").target(struct).build()
        val set =
            ListShape.builder()
                .id("foo.bar#Records")
                .member(setMember)
                .addTrait(SparseTrait())
                .build()
        val model =
            Model.assembler()
                .putProperty(ModelAssembler.ALLOW_UNKNOWN_TRAITS, true)
                .addShapes(set, setMember, struct)
                .assemble()
                .unwrap()

        val provider: SymbolProvider = testSymbolProvider(model)
        val setSymbol = provider.toSymbol(set)
        setSymbol.rustType().render(false) shouldBe "Vec::<Option<Record>>"
        setSymbol.referenceClosure().map { it.name } shouldBe listOf("Vec", "Option", "Record")
    }

    @Test
    fun `create timestamps`() {
        val memberBuilder = MemberShape.builder().id("foo.bar#MyStruct\$someField").target("smithy.api#Timestamp")
        val member = memberBuilder.build()
        val struct =
            StructureShape.builder()
                .id("foo.bar#MyStruct")
                .addMember(member)
                .build()
        val model =
            Model.assembler()
                .addShapes(struct, member)
                .assemble()
                .unwrap()
        val provider: SymbolProvider = testSymbolProvider(model)
        val sym = provider.toSymbol(member)
        sym.rustType().render(false) shouldBe "Option<DateTime>"
        sym.referenceClosure().map { it.name } shouldContain "DateTime"
        sym.references[0].dependencies.shouldNotBeEmpty()
    }

    @Test
    fun `creates operations`() {
        val model =
            """
            namespace smithy.example

            @idempotent
            @http(method: "PUT", uri: "/{bucketName}/{key}", code: 200)
            operation PutObject {
                input: PutObjectInput
            }

            structure PutObjectInput {
                // Sent in the URI label named "key".
                @required
                @httpLabel
                key: String,

                // Sent in the URI label named "bucketName".
                @required
                @httpLabel
                bucketName: String,

                // Sent in the X-Foo header
                @httpHeader("X-Foo")
                foo: String,

                // Sent in the query string as paramName
                @httpQuery("paramName")
                someValue: String,

                // Sent in the body
                data: Blob,

                // Sent in the body
                additional: String,
            }
            """.asSmithyModel()
        val symbol = testSymbolProvider(model).toSymbol(model.expectShape(ShapeId.from("smithy.example#PutObject")))
        symbol.definitionFile shouldBe "src/test_operation.rs"
        symbol.name shouldBe "PutObject"
    }

    @Test
    fun `handles bigInteger shapes`() {
        val model =
            """
            namespace test

            bigInteger MyBigInt
            """.asSmithyModel()
        val provider = testSymbolProvider(model)
        val sym = provider.toSymbol(model.expectShape(ShapeId.from("test#MyBigInt")))
        sym.rustType().render(false) shouldBe "BigInteger"
        sym.namespace shouldBe "::aws_smithy_types"
    }

    @Test
    fun `handles bigDecimal shapes`() {
        val model =
            """
            namespace test

            bigDecimal MyBigDecimal
            """.asSmithyModel()
        val provider = testSymbolProvider(model)
        val sym = provider.toSymbol(model.expectShape(ShapeId.from("test#MyBigDecimal")))
        sym.rustType().render(false) shouldBe "BigDecimal"
        sym.namespace shouldBe "::aws_smithy_types"
    }
}
