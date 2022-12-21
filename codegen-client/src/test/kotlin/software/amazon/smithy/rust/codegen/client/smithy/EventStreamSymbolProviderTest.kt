/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.EventStreamSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitor
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.TestRuntimeConfig
import software.amazon.smithy.rust.codegen.core.testutil.TestSymbolVisitorConfig
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel

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
            """.asSmithyModel(),
        )

        val service = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val provider = EventStreamSymbolProvider(TestRuntimeConfig, SymbolVisitor(model, service, TestSymbolVisitorConfig), model, CodegenTarget.CLIENT)

        // Look up the synthetic input/output rather than the original input/output
        val inputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationInput\$inputStream")) as MemberShape
        val outputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationOutput\$outputStream")) as MemberShape

        val inputType = provider.toSymbol(inputStream).rustType()
        val outputType = provider.toSymbol(outputStream).rustType()

        inputType shouldBe RustType.Opaque("EventStreamSender<crate::model::SomeStream, crate::error::SomeStreamError>", "aws_smithy_http::event_stream")
        outputType shouldBe RustType.Opaque("Receiver<crate::model::SomeStream, crate::error::SomeStreamError>", "aws_smithy_http::event_stream")
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
            """.asSmithyModel(),
        )

        val service = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val provider = EventStreamSymbolProvider(TestRuntimeConfig, SymbolVisitor(model, service, TestSymbolVisitorConfig), model, CodegenTarget.CLIENT)

        // Look up the synthetic input/output rather than the original input/output
        val inputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationInput\$inputStream")) as MemberShape
        val outputStream = model.expectShape(ShapeId.from("test.synthetic#TestOperationOutput\$outputStream")) as MemberShape

        val inputType = provider.toSymbol(inputStream).rustType()
        val outputType = provider.toSymbol(outputStream).rustType()

        inputType shouldBe RustType.Option(RustType.Opaque("NotStreaming", "crate::model"))
        outputType shouldBe RustType.Option(RustType.Opaque("NotStreaming", "crate::model"))
    }
}
