/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.testutil.TestSymbolVisitorConfig
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel

internal class PythonServerSymbolProviderTest {
    @Test
    fun `python symbol provider rewrites timestamp shape symbol`() {
        val model = """
            namespace test

            structure TimestampStruct {
                inner: Timestamp
            }

            list TimestampList {
                member: Timestamp
            }

            set TimestampSet {
                member: Timestamp
            }

            map TimestampMap {
                key: String,
                value: Timestamp
            }
        """.asSmithyModel()
        val provider = PythonServerSymbolProvider(SymbolVisitor(model, null, TestSymbolVisitorConfig), model)

        // Struct test
        val timestamp = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampStruct\$inner"))).rustType()
        timestamp shouldBe RustType.Opaque("DateTime", "aws_smithy_http_server_python::types")

        // List test
        val timestampList = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampList"))).rustType()
        timestampList shouldBe RustType.Vec(RustType.Opaque("DateTime", "aws_smithy_http_server_python::types"))

        // Set test
        val timestampSet = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampSet"))).rustType()
        timestampSet shouldBe RustType.Vec(RustType.Opaque("DateTime", "aws_smithy_http_server_python::types"))

        // Map test
        val timestampMap = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampMap"))).rustType()
        timestampMap shouldBe RustType.HashMap(RustType.String, RustType.Opaque("DateTime", "aws_smithy_http_server_python::types"))
    }

    @Test
    fun `python symbol provider rewrites blob shape symbol`() {
        val model = """
            namespace test

            structure BlobStruct {
                inner: Blob
            }

            list BlobList {
                member: Blob
            }

            set BlobSet {
                member: Blob
            }

            map BlobMap {
                key: String,
                value: Blob
            }
        """.asSmithyModel()
        val provider = PythonServerSymbolProvider(SymbolVisitor(model, null, TestSymbolVisitorConfig), model)

        // Struct test
        val blob = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobStruct\$inner"))).rustType()
        blob shouldBe RustType.Opaque("Blob", "aws_smithy_http_server_python::types")

        // List test
        val blobList = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobList"))).rustType()
        blobList shouldBe RustType.Vec(RustType.Opaque("Blob", "aws_smithy_http_server_python::types"))

        // Set test
        val blobSet = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobSet"))).rustType()
        blobSet shouldBe RustType.Vec(RustType.Opaque("Blob", "aws_smithy_http_server_python::types"))

        // Map test
        val blobMap = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobMap"))).rustType()
        blobMap shouldBe RustType.HashMap(RustType.String, RustType.Opaque("Blob", "aws_smithy_http_server_python::types"))
    }
}
