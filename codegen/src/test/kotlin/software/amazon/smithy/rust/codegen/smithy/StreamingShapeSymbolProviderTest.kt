/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
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
        val modelWithOperationTraits = OperationNormalizer.transform(model)
        val symbolProvider = testSymbolProvider(modelWithOperationTraits)
        symbolProvider.toSymbol(modelWithOperationTraits.lookup<MemberShape>("test.synthetic#GenerateSpeechOutput\$data")).name shouldBe ("ByteStream")
        symbolProvider.toSymbol(modelWithOperationTraits.lookup<MemberShape>("test.synthetic#GenerateSpeechInput\$data")).name shouldBe ("ByteStream")
    }

    @Test
    fun `streaming members have a default`() {
        val modelWithOperationTraits = OperationNormalizer.transform(model)
        val symbolProvider = testSymbolProvider(modelWithOperationTraits)

        val outputSymbol = symbolProvider.toSymbol(modelWithOperationTraits.lookup<MemberShape>("test.synthetic#GenerateSpeechOutput\$data"))
        val inputSymbol = symbolProvider.toSymbol(modelWithOperationTraits.lookup<MemberShape>("test.synthetic#GenerateSpeechInput\$data"))
        // Ensure that users don't need to set an input
        outputSymbol.defaultValue() shouldBe Default.RustDefault
        inputSymbol.defaultValue() shouldBe Default.RustDefault
    }
}
