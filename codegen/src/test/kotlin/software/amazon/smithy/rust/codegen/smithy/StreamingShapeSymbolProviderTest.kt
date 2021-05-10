/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.util.lookup

internal class StreamingShapeSymbolProviderTest {
    val model = """
        namespace test
        operation GenerateSpeech {
            output: GenerateSpeechOutput,
            input: GenerateSpeechOutput
        }

        structure GenerateSpeechOutput {
            data: BlobStream
        }

        @streaming
        blob BlobStream

    """.asSmithyModel()

    @Test
    fun `generates a byte stream on streaming output`() {
        // we could test exactly the streaming shape symbol provider, but we actually care about is the full stack
        // "doing the right thing"
        val modelWithOperationTraits =
            OperationNormalizer(model).transformModel(OperationNormalizer.NoBody, OperationNormalizer.NoBody)
        val symbolProvider = testSymbolProvider(modelWithOperationTraits)
        symbolProvider.toSymbol(modelWithOperationTraits.lookup<MemberShape>("test#GenerateSpeechOutput\$data")).name shouldBe ("byte_stream::ByteStream")
        symbolProvider.toSymbol(modelWithOperationTraits.lookup<MemberShape>("test#GenerateSpeechInput\$data"))
            .rustType() shouldBe (RustType.Option(RustType.Opaque("Blob", "smithy_types")))
    }

    // This is coming up soon, but for now, we only generate streaming binary output
    @Test
    fun `do not generate a byte stream in streaming input`() {
        val modelWithOperationTraits =
            OperationNormalizer(model).transformModel(OperationNormalizer.NoBody, OperationNormalizer.NoBody)
        val symbolProvider = testSymbolProvider(modelWithOperationTraits)
        symbolProvider.toSymbol(modelWithOperationTraits.lookup<MemberShape>("test#GenerateSpeechInput\$data"))
            .rustType() shouldBe (RustType.Option(RustType.Opaque("Blob", "smithy_types")))
    }
}
