/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy.generators

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.server.python.smithy.PythonServerSymbolVisitor
import software.amazon.smithy.rust.codegen.server.smithy.testutil.ServerTestRustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings

internal class PythonServerSymbolProviderTest {
    private val pythonBlobType = RustType.Opaque("Blob", "::aws_smithy_http_server_python::types")
    private val pythonTimestampType = RustType.Opaque("DateTime", "::aws_smithy_http_server_python::types")

    @Test
    fun `python symbol provider rewrites timestamp shape symbol`() {
        val model =
            """
            namespace test

            structure TimestampStruct {
                @required
                inner: Timestamp
            }

            structure TimestampStructOptional {
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
        val provider =
            PythonServerSymbolVisitor(serverTestRustSettings(), model, null, ServerTestRustSymbolProviderConfig)

        // Struct test
        val timestamp = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampStruct\$inner"))).rustType()
        timestamp shouldBe pythonTimestampType

        // Optional struct test
        val optionalTimestamp = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampStructOptional\$inner"))).rustType()
        optionalTimestamp shouldBe RustType.Option(pythonTimestampType)

        // List test
        val timestampList = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampList"))).rustType()
        timestampList shouldBe RustType.Vec(pythonTimestampType)

        // Set test
        val timestampSet = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampSet"))).rustType()
        timestampSet shouldBe RustType.Vec(pythonTimestampType)

        // Map test
        val timestampMap = provider.toSymbol(model.expectShape(ShapeId.from("test#TimestampMap"))).rustType()
        timestampMap shouldBe RustType.HashMap(RustType.String, pythonTimestampType)
    }

    @Test
    fun `python symbol provider rewrites blob shape symbol`() {
        val model =
            """
            namespace test

            structure BlobStruct {
                @required
                inner: Blob
            }

            structure BlobStructOptional {
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
        val provider =
            PythonServerSymbolVisitor(serverTestRustSettings(), model, null, ServerTestRustSymbolProviderConfig)

        // Struct test
        val blob = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobStruct\$inner"))).rustType()
        blob shouldBe pythonBlobType

        // Optional struct test
        val optionalBlob = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobStructOptional\$inner"))).rustType()
        optionalBlob shouldBe RustType.Option(pythonBlobType)

        // List test
        val blobList = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobList"))).rustType()
        blobList shouldBe RustType.Vec(pythonBlobType)

        // Set test
        val blobSet = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobSet"))).rustType()
        blobSet shouldBe RustType.Vec(pythonBlobType)

        // Map test
        val blobMap = provider.toSymbol(model.expectShape(ShapeId.from("test#BlobMap"))).rustType()
        blobMap shouldBe RustType.HashMap(RustType.String, pythonBlobType)
    }
}
