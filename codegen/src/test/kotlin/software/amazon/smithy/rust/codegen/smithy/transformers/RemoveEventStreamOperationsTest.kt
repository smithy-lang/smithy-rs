/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.transformers

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel

internal class RemoveEventStreamOperationsTest {
    @Test
    fun `event stream operations are removed from the model`() {
        val model = """
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
        val transformed = RemoveEventStreamOperations.transform(model)
        transformed.expectShape(ShapeId.from("test#BlobStream"))
        transformed.getShape(ShapeId.from("test#EventStream")).shouldBe(java.util.Optional.empty())
    }
}
