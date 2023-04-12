/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.transformers

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenConfig
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import java.util.Optional

internal class RemoveEventStreamOperationsTest {
    private val model = """
        namespace test
        operation EventStream {
            input: StreamingInput,
        }

        operation BlobStream{
            input: BlobInput
        }

        structure BlobInput {
            blob: StreamingBlob
        }

        @streaming
        blob StreamingBlob

        structure StreamingInput {
            payload: Event
        }

        @streaming
        union Event {
            s: Foo
        }

        structure Foo {}
    """.asSmithyModel()

    @Test
    fun `remove event stream ops from services that are not in the allow list`() {
        val transformed = RemoveEventStreamOperations.transform(
            model,
            testClientRustSettings(
                codegenConfig = ClientCodegenConfig(eventStreamAllowList = setOf("not-test-module")),
            ),
        )
        transformed.expectShape(ShapeId.from("test#BlobStream"))
        transformed.getShape(ShapeId.from("test#EventStream")) shouldBe Optional.empty()
    }

    @Test
    fun `keep event stream ops from services that are in the allow list`() {
        val transformed = RemoveEventStreamOperations.transform(
            model,
            testClientRustSettings(
                codegenConfig = ClientCodegenConfig(eventStreamAllowList = setOf("test-module")),
            ),
        )
        transformed.expectShape(ShapeId.from("test#BlobStream"))
        transformed.getShape(ShapeId.from("test#EventStream")) shouldNotBe Optional.empty<Shape>()
    }
}
