/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.transformers

import io.kotest.matchers.shouldBe
import io.kotest.matchers.shouldNotBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.ClientCodegenConfig
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import java.util.Optional

internal class RemoveEventStreamOperationsTest {
    private fun model(
        protocol: String,
        rest: Boolean,
    ): Model {
        val httpPayload =
            if (rest) {
                "@httpPayload"
            } else {
                ""
            }
        return """
            namespace test

            use aws.protocols#awsJson1_0
            use aws.protocols#restJson1
            use aws.protocols#restXml

            @$protocol
            service TestService {
                operations: [EventStream, BlobStream],
            }

            operation EventStream {
                input: StreamingInput,
            }

            operation BlobStream{
                input: BlobInput
            }

            @input
            structure BlobInput {
                $httpPayload
                blob: StreamingBlob
            }

            @streaming
            blob StreamingBlob

            @input
            structure StreamingInput {
                $httpPayload
                payload: Event
            }

            @streaming
            union Event {
                s: Foo
            }

            structure Foo {}
        """.asSmithyModel()
    }

    @Test
    fun `remove event stream ops from services that are not in the allow list`() {
        val transformed =
            RemoveEventStreamOperations.transform(
                model(protocol = "awsJson1_0", rest = false),
                testClientRustSettings(
                    service = ShapeId.from("test#TestService"),
                    codegenConfig = ClientCodegenConfig(eventStreamAllowList = setOf("not-test-module")),
                ),
            )
        transformed.expectShape(ShapeId.from("test#BlobStream"))
        transformed.getShape(ShapeId.from("test#EventStream")) shouldBe Optional.empty()
    }

    @Test
    fun `keep event stream ops from services that are in the allow list`() {
        val transformed =
            RemoveEventStreamOperations.transform(
                model(protocol = "awsJson1_0", rest = false),
                testClientRustSettings(
                    service = ShapeId.from("test#TestService"),
                    codegenConfig = ClientCodegenConfig(eventStreamAllowList = setOf("test-module")),
                ),
            )
        transformed.expectShape(ShapeId.from("test#BlobStream"))
        transformed.getShape(ShapeId.from("test#EventStream")) shouldNotBe Optional.empty<Shape>()
    }

    @Test
    fun `keep event stream ops for rest services`() {
        var transformed =
            RemoveEventStreamOperations.transform(
                model(protocol = "restJson1", rest = true),
                testClientRustSettings(
                    service = ShapeId.from("test#TestService"),
                    codegenConfig = ClientCodegenConfig(eventStreamAllowList = setOf()),
                ),
            )
        transformed.expectShape(ShapeId.from("test#BlobStream"))
        transformed.getShape(ShapeId.from("test#EventStream")) shouldNotBe Optional.empty<Shape>()

        transformed =
            RemoveEventStreamOperations.transform(
                model(protocol = "restXml", rest = true),
                testClientRustSettings(
                    service = ShapeId.from("test#TestService"),
                    codegenConfig = ClientCodegenConfig(eventStreamAllowList = setOf()),
                ),
            )
        transformed.expectShape(ShapeId.from("test#BlobStream"))
        transformed.getShape(ShapeId.from("test#EventStream")) shouldNotBe Optional.empty<Shape>()
    }
}
