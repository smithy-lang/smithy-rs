/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.CsvSource
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.testutil.TestWorkspace
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.compileAndTest
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.lookup

internal class CustomSerializerGeneratorTest {
    private val model = """
    namespace test
    structure S {
        timestamp: Timestamp,
        string: String,
        blob: Blob,
        blobList: BlobList,
        sparseBlobList: SparseBlobList
    }
    list BlobList {
        member: Blob
    }
    @sparse
    list SparseBlobList {
        member: Blob
    }

    map Nested {
        key: String,
        value: BlobList
    }

    structure TopLevel {
        member: Nested
    }
    """.asSmithyModel()
    private val provider = testSymbolProvider(model)

    @Test
    fun `generate correct function names`() {
        val serializerBuilder = CustomSerializerGenerator(provider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        serializerBuilder.serializerFor(model.lookup("test#S\$timestamp"))!!.name shouldBe "stdoptionoptionsmithytypesinstant_epoch_seconds_ser"
        serializerBuilder.serializerFor(model.lookup("test#S\$blob"))!!.name shouldBe "stdoptionoptionsmithytypesblob_ser"
        serializerBuilder.deserializerFor(model.lookup("test#S\$blob"))!!.name shouldBe "stdoptionoptionsmithytypesblob_deser"
        serializerBuilder.deserializerFor(model.lookup("test#S\$string")) shouldBe null
    }

    private fun checkDeserializer(builder: CustomSerializerGenerator, shapeId: String) {
        val symbol = builder.deserializerFor(model.lookup(shapeId))
        check(symbol != null) { "For $shapeId, expected a custom deserializer" }
        checkSymbol(symbol)
    }

    private fun checkSerializer(builder: CustomSerializerGenerator, shapeId: String) {
        val symbol = builder.serializerFor(model.lookup(shapeId))
        check(symbol != null) { "For $shapeId, expected a custom serializer" }
        checkSymbol(symbol)
    }

    private fun checkSymbol(symbol: RuntimeType) {
        val writer = TestWorkspace.testProject(provider)
        writer.lib {
            it.rust(
                """
                    fn foo() {
                        // commented out so that we generate the import & inject the serializer
                        // but I don't want to deal with getting the argument to compile
                        // let _ = #T();
                    }
                """,
                symbol
            )
        }
        println("file:///${writer.baseDir}/src/serde_util.rs")
        writer.compileAndTest()
    }

    @ParameterizedTest(name = "{index} ==> ''{0}''")
    @CsvSource(
        "timestamp",
        "blob",
        "blobList",
        "sparseBlobList"
    )
    fun `generate basic deserializers that compile`(memberName: String) {
        val serializerBuilder = CustomSerializerGenerator(provider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        checkDeserializer(serializerBuilder, "test#S\$$memberName")
        checkSerializer(serializerBuilder, "test#S\$$memberName")
    }

    @Test
    fun `support deeply nested structures`() {
        val serializerBuilder = CustomSerializerGenerator(provider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        checkDeserializer(serializerBuilder, "test#TopLevel\$member")
        checkSerializer(serializerBuilder, "test#TopLevel\$member")
    }

    @Test
    fun `generate deserializers for boxed shapes`() {
        val boxingProvider = object : WrappingSymbolProvider(provider) {
            override fun toSymbol(shape: Shape): Symbol {
                return provider.toSymbol(shape).makeRustBoxed()
            }
        }
        val serializerBuilder = CustomSerializerGenerator(boxingProvider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        checkSerializer(serializerBuilder, "test#S\$timestamp")
        checkDeserializer(serializerBuilder, "test#S\$timestamp")
    }
}
