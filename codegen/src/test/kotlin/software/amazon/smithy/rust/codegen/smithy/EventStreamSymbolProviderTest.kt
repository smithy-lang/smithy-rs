/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.testutil.TestSymbolVisitorConfig
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel

class EventStreamSymbolProviderTest {
    @Test
    fun `it should adjust types for operations with event streams`() {
        // Transform the model so that it has synthetic inputs/outputs
        val model = OperationNormalizer.transform(
            """
            namespace test

            structure Something { stuff: Blob }

            @streaming
            union SomeStream {
                Something: Something,
            }

            structure TestInput { inputStream: SomeStream }
            structure TestOutput { outputStream: SomeStream }
            operation TestOperation {
                input: TestInput,
                output: TestOutput,
            }
            service TestService { version: "123", operations: [TestOperation] }
            """.asSmithyModel()
        )

        val service = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val provider = EventStreamSymbolProvider(TestRuntimeConfig, SymbolVisitor(model, service, TestSymbolVisitorConfig), model)

        // Look up the synthetic input/output rather than the original input/output
        val inputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationInput\$inputStream")) as MemberShape
        val outputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationOutput\$outputStream")) as MemberShape

        val inputType = provider.toSymbol(inputStream).rustType()
        val outputType = provider.toSymbol(outputStream).rustType()

        inputType shouldBe RustType.Opaque("EventStreamInput<crate::model::SomeStream>", "aws_smithy_http::event_stream")
        outputType shouldBe RustType.Opaque("Receiver<crate::model::SomeStream, crate::error::TestOperationError>", "aws_smithy_http::event_stream")
    }

    @Test
    fun `it should leave alone types for operations without event streams`() {
        val model = OperationNormalizer.transform(
            """
            namespace test

            structure Something { stuff: Blob }

            union NotStreaming {
                Something: Something,
            }

            structure TestInput { inputStream: NotStreaming }
            structure TestOutput { outputStream: NotStreaming }
            operation TestOperation {
                input: TestInput,
                output: TestOutput,
            }
            service TestService { version: "123", operations: [TestOperation] }
            """.asSmithyModel()
        )

        val service = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val provider = EventStreamSymbolProvider(TestRuntimeConfig, SymbolVisitor(model, service, TestSymbolVisitorConfig), model)

        // Look up the synthetic input/output rather than the original input/output
        val inputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationInput\$inputStream")) as MemberShape
        val outputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationOutput\$outputStream")) as MemberShape

        val inputType = provider.toSymbol(inputStream).rustType()
        val outputType = provider.toSymbol(outputStream).rustType()

        inputType shouldBe RustType.Option(RustType.Opaque("NotStreaming", "crate::model"))
        outputType shouldBe RustType.Option(RustType.Opaque("NotStreaming", "crate::model"))
    }
}
