/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.protocols.eventstream

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.CombinedClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.ErrorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.OperationErrorGenerator
import software.amazon.smithy.rust.codegen.client.testutil.testClientRustSettings
import software.amazon.smithy.rust.codegen.client.testutil.testSymbolProvider
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestRequirements
import software.amazon.smithy.rust.codegen.core.util.expectTrait
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
        testClientRustSettings(),
        CombinedClientCodegenDecorator(emptyList()),
    )

    override fun renderBuilderForShape(
        rustCrate: RustCrate,
        writer: RustWriter,
        codegenContext: ClientCodegenContext,
        shape: StructureShape,
    ) {
        BuilderGenerator(codegenContext.model, codegenContext.symbolProvider, shape, emptyList()).apply {
            render(writer)
        }
        writer.implBlock(codegenContext.symbolProvider.toSymbol(shape)) {
            BuilderGenerator.renderConvenienceMethod(writer, codegenContext.symbolProvider, shape)
        }
    }

    override fun renderOperationError(
        writer: RustWriter,
        model: Model,
        symbolProvider: RustSymbolProvider,
        operationOrEventStream: Shape,
    ) {
        OperationErrorGenerator(model, symbolProvider, operationOrEventStream, emptyList()).render(writer)
    }

    override fun renderError(
        rustCrate: RustCrate,
        writer: RustWriter,
        codegenContext: ClientCodegenContext,
        shape: StructureShape,
    ) {
        val errorTrait = shape.expectTrait<ErrorTrait>()
        val errorGenerator = ErrorGenerator(
            codegenContext.model,
            codegenContext.symbolProvider,
            shape,
            errorTrait,
            emptyList(),
        )
        rustCrate.useShapeWriter(shape) {
            errorGenerator.renderStruct(this)
        }
        rustCrate.withModule(codegenContext.symbolProvider.moduleForBuilder(shape)) {
            errorGenerator.renderBuilder(this)
        }
    }
}
