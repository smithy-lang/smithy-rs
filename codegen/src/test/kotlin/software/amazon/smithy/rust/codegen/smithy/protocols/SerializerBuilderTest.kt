/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.protocols

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.TimestampFormatTrait
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.smithy.WrappingSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.makeRustBoxed
import software.amazon.smithy.rust.codegen.util.lookup
import software.amazon.smithy.rust.testutil.TestWorkspace
import software.amazon.smithy.rust.testutil.asSmithyModel
import software.amazon.smithy.rust.testutil.compileAndTest
import software.amazon.smithy.rust.testutil.testSymbolProvider

internal class SerializerBuilderTest {
    private val model = """
    namespace test
    structure S {
        ts: Timestamp,
        s: String,
        b: Blob,
        bl: BlobList,
        sbl: SparseBlobList
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
        val serializerBuilder = SerializerBuilder(provider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        serializerBuilder.serializerFor(model.lookup("test#S\$ts"))!!.name shouldBe "stdoptionoptioninstant_epoch_seconds_ser"
        serializerBuilder.serializerFor(model.lookup("test#S\$b"))!!.name shouldBe "stdoptionoptionblob_ser"
        serializerBuilder.deserializerFor(model.lookup("test#S\$b"))!!.name shouldBe "stdoptionoptionblob_deser"
        serializerBuilder.deserializerFor(model.lookup("test#S\$s")) shouldBe null
    }

    @Test
    fun `generate basic deserializers that compile`() {
        val serializerBuilder = SerializerBuilder(provider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        val timestamp = serializerBuilder.deserializerFor(model.lookup("test#S\$ts"))!!
        val blob = serializerBuilder.deserializerFor(model.lookup("test#S\$b"))!!
        val blobList = serializerBuilder.deserializerFor(model.lookup("test#S\$bl"))!!
        val sparseBlobList = serializerBuilder.deserializerFor(model.lookup("test#S\$sbl"))!!
        val writer = TestWorkspace.testProject(provider)
        writer.useFileWriter("src/lib.rs", "crate::lib") {
            it.rust(
                """
                fn foo() {
                    // commented out so that we generate the import & inject the serializer
                    // but I don't want to deal with getting the argument to compile
                    // let _ = #T();
                    // let _ = #T();
                    // let _ = #T();
                    // let _ = #T();
                }
            """,
                timestamp, blob, blobList, sparseBlobList
            )
        }
        println("file:///${writer.baseDir}/src/serde_util.rs")
        writer.compileAndTest()
    }

    @Test
    fun `support deeply nested structures`() {
        val serializerBuilder = SerializerBuilder(provider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        val timestamp = serializerBuilder.deserializerFor(model.lookup("test#TopLevel\$member"))!!
        val writer = TestWorkspace.testProject(provider)
        writer.useFileWriter("src/lib.rs", "crate::lib") {
            it.rust(
                """
                fn foo() {
                    // commented out so that we generate the import & inject the serializer
                    // but I don't want to deal with getting the argument to compile
                    // let _ = #T();
                }
            """,
                timestamp
            )
        }
        println("file:///${writer.baseDir}/src/serde_util.rs")
        writer.compileAndTest()
    }

    @Test
    fun `generate deserializers for boxed shapes`() {
        val boxingProvider = object : WrappingSymbolProvider(provider) {
            override fun toSymbol(shape: Shape): Symbol {
                return provider.toSymbol(shape).makeRustBoxed()
            }
        }
        val serializerBuilder = SerializerBuilder(boxingProvider, model, TimestampFormatTrait.Format.EPOCH_SECONDS)
        val timestamp = serializerBuilder.deserializerFor(model.lookup("test#S\$ts"))!!
        val writer = TestWorkspace.testProject(provider)
        writer.useFileWriter("src/lib.rs", "crate::lib") {
            it.rust(
                """
                fn foo() {
                    // commented out so that we generate the import & inject the serializer
                    // but I don't want to deal with getting the argument to compile
                    // let _ = #T();
                }
            """,
                timestamp
            )
        }
        println("file:///${writer.baseDir}/src/serde_util.rs")
        writer.compileAndTest()
    }
}
