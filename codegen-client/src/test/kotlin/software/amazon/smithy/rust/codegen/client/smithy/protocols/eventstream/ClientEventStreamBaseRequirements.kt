/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.testutil.clientTestRustSettings
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.OperationErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestRequirements
import java.util.stream.Stream

class TestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES.map { Arguments.of(it) }.stream()
}

abstract class ClientEventStreamBaseRequirements : EventStreamTestRequirements<ClientCodegenContext> {
    override fun createCodegenContext(
        model: Model,
        serviceShape: ServiceShape,
        protocolShapeId: ShapeId,
        codegenTarget: CodegenTarget,
    ): ClientCodegenContext = ClientCodegenContext(
        model,
        testSymbolProvider(model),
        serviceShape,
        protocolShapeId,
        clientTestRustSettings(),
        CombinedClientCodegenDecorator(emptyList()),
    )

    override fun renderBuilderForShape(
        writer: RustWriter,
        codegenContext: ClientCodegenContext,
        shape: StructureShape,
    ) {
        BuilderGenerator(codegenContext.model, codegenContext.symbolProvider, shape).apply {
            render(writer)
            writer.implBlock(shape, codegenContext.symbolProvider) {
                renderConvenienceMethod(writer)
            }
        }
    }

    override fun renderOperationError(
        writer: RustWriter,
        model: Model,
        symbolProvider: RustSymbolProvider,
        operationSymbol: Symbol,
        errors: List<StructureShape>,
    ) {
        OperationErrorGenerator(model, symbolProvider, operationSymbol, errors).render(writer)
    }
}
