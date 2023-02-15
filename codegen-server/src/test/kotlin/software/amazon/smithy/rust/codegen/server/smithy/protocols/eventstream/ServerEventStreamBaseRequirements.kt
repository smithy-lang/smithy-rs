/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.protocols.eventstream

import org.junit.jupiter.api.extension.ExtensionContext
import org.junit.jupiter.params.provider.Arguments
import org.junit.jupiter.params.provider.ArgumentsProvider
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplGenerator
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestModels
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamTestRequirements
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenConfig
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.customizations.SmithyValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGeneratorWithoutPublicConstrainedTypes
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerOperationErrorGenerator
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.testutil.serverTestRustSettings
import java.util.stream.Stream

data class TestCase(
    val eventStreamTestCase: EventStreamTestModels.TestCase,
    val publicConstrainedTypes: Boolean,
) {
    override fun toString(): String = "$eventStreamTestCase, publicConstrainedTypes = $publicConstrainedTypes"
}

class TestCasesProvider : ArgumentsProvider {
    override fun provideArguments(context: ExtensionContext?): Stream<out Arguments> =
        EventStreamTestModels.TEST_CASES
            .flatMap { testCase ->
                listOf(
                    TestCase(testCase, publicConstrainedTypes = false),
                    TestCase(testCase, publicConstrainedTypes = true),
                )
            }.map { Arguments.of(it) }.stream()
}

abstract class ServerEventStreamBaseRequirements : EventStreamTestRequirements<ServerCodegenContext> {
    abstract val publicConstrainedTypes: Boolean

    override fun createCodegenContext(
        model: Model,
        serviceShape: ServiceShape,
        protocolShapeId: ShapeId,
        codegenTarget: CodegenTarget,
    ): ServerCodegenContext = serverTestCodegenContext(
        model,
        serviceShape,
        serverTestRustSettings(
            codegenConfig = ServerCodegenConfig(publicConstrainedTypes = publicConstrainedTypes),
        ),
        protocolShapeId,
    )

    override fun renderBuilderForShape(
        rustCrate: RustCrate,
        writer: RustWriter,
        codegenContext: ServerCodegenContext,
        shape: StructureShape,
    ) {
        val validationExceptionConversionGenerator = SmithyValidationExceptionConversionGenerator(codegenContext)
        if (codegenContext.settings.codegenConfig.publicConstrainedTypes) {
            // FZ rebase
            ServerBuilderGenerator(codegenContext, shape, validationExceptionConversionGenerator).apply {
                render(rustCrate, writer)
                writer.implBlock(codegenContext.symbolProvider.toSymbol(shape)) {
                    renderConvenienceMethod(writer)
                }
            }
        } else {
            ServerBuilderGeneratorWithoutPublicConstrainedTypes(codegenContext, shape, validationExceptionConversionGenerator).apply {
                render(rustCrate, writer)
                writer.implBlock(codegenContext.symbolProvider.toSymbol(shape)) {
                    renderConvenienceMethod(writer)
                }
            }
        }
    }

    override fun renderOperationError(
        writer: RustWriter,
        model: Model,
        symbolProvider: RustSymbolProvider,
        operationOrEventStream: Shape,
    ) {
        ServerOperationErrorGenerator(model, symbolProvider, operationOrEventStream).render(writer)
    }

    override fun renderError(
        writer: RustWriter,
        codegenContext: ServerCodegenContext,
        shape: StructureShape,
    ) {
        StructureGenerator(codegenContext.model, codegenContext.symbolProvider, writer, shape, listOf()).render()
        ErrorImplGenerator(
            codegenContext.model,
            codegenContext.symbolProvider,
            writer,
            shape,
            shape.getTrait()!!,
            listOf(),
        ).render(CodegenTarget.SERVER)
        // FZ TODO()
        //TODO()
        //renderBuilderForShape(writer, codegenContext, shape)
    }
}
