/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.params.ParameterizedTest
import org.junit.jupiter.params.provider.ArgumentsSource
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.protocols.parse.EventStreamUnmarshallerGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestTools
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestVariety
import software.amazon.smithy.rust.codegen.core.testutil.TestEventStreamProject

class ClientEventStreamUnmarshallerGeneratorTest {
    @ParameterizedTest
    @ArgumentsSource(TestCasesProvider::class)
    fun test(testCase: EventStreamTestModels.TestCase) {
        EventStreamTestTools.runTestCase(
            testCase,
            object : ClientEventStreamBaseRequirements() {
                override fun renderGenerator(
                    codegenContext: ClientCodegenContext,
                    project: TestEventStreamProject,
                    protocol: Protocol,
                ): RuntimeType {
                    fun builderSymbol(shape: StructureShape): Symbol = shape.builderSymbol(codegenContext.symbolProvider)
                    return EventStreamUnmarshallerGenerator(
                        protocol,
                        codegenContext,
                        project.operationShape,
                        project.streamShape,
                        ::builderSymbol,
                    ).render()
                }
            },
            CodegenTarget.CLIENT,
            EventStreamTestVariety.Unmarshall,
        )
    }
}
