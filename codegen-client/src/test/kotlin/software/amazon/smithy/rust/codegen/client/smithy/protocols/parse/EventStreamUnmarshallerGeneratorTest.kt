/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.parse

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import java.util.stream.Stream

class UnmarshallTestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES.map { Arguments.of(it) }.stream()
}

class EventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(UnmarshallTestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        EventStreamTestTools.runTestCase(
            testCase,
            CodegenTarget.CLIENT,
            { model -> testSymbolProvider(model) },
            { codegenContext, test, protocol ->
                fun builderSymbol(shape: StructureShape): Symbol = shape.builderSymbol(codegenContext.symbolProvider)
                EventStreamUnmarshallerGenerator(
                    protocol,
                    codegenContext,
                    test.operationShape,
                    test.streamShape,
                    ::builderSymbol,
                ).render()
            },
            marshall = false,
        )
    }
}
