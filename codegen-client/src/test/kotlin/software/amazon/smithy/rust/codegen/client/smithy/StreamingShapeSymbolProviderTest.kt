/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import io.kotest.matchers.shouldBe
import org.junit.jupiter.api.Test
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.RustType
import software.amazon.smithy.rust.codegen.core.smithy.Default
import software.amazon.smithy.rust.codegen.core.smithy.defaultValue
import software.amazon.smithy.rust.codegen.core.smithy.rustType
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.asSmithyModel
import software.amazon.smithy.rust.codegen.core.util.lookup

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
        modelWithOperationTraits.lookup<MemberShape>("test.synthetic#GenerateSpeechOutput\$data").also { shape ->
            symbolProvider.toSymbol(shape).also { symbol ->
                symbol.name shouldBe "data"
                symbol.rustType() shouldBe RustType.Opaque("ByteStream", "::aws_smithy_http::byte_stream")
            }
        }
        modelWithOperationTraits.lookup<MemberShape>("test.synthetic#GenerateSpeechInput\$data").also { shape ->
            symbolProvider.toSymbol(shape).also { symbol ->
                symbol.name shouldBe "data"
                symbol.rustType() shouldBe RustType.Opaque("ByteStream", "::aws_smithy_http::byte_stream")
            }
        }
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
