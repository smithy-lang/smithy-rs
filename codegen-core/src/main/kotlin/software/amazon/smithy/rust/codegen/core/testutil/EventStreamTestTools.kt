/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.testutil

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ErrorsModule
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.CombinedErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ServerCombinedErrorGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.renderUnknownVariant
import software.amazon.smithy.rust.codegen.core.smithy.protocols.Protocol
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamMarshallTestCases.writeMarshallTestCases
import software.amazon.smithy.rust.codegen.core.testutil.EventStreamUnmarshallTestCases.writeUnmarshallTestCases
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.lookup
import software.amazon.smithy.rust.codegen.core.util.outputShape
import kotlin.streams.toList

data class TestEventStreamProject(
    val model: Model,
    val serviceShape: ServiceShape,
    val operationShape: OperationShape,
    val streamShape: UnionShape,
    val symbolProvider: RustSymbolProvider,
    val project: TestWriterDelegator,
)

enum class EventStreamTestVariety {
    Marshall,
    Unmarshall
}

interface EventStreamTestRequirements<C : CodegenContext> {
    /** Create a codegen context for the tests */
    fun createCodegenContext(
        model: Model,
        symbolProvider: RustSymbolProvider,
        serviceShape: ServiceShape,
        protocolShapeId: ShapeId,
        codegenTarget: CodegenTarget,
    ): C

    /** Create a symbol provider for the tests */
    fun createSymbolProvider(model: Model): RustSymbolProvider

    /** Render the event stream marshall/unmarshall code generator */
    fun renderGenerator(
        codegenContext: C,
        project: TestEventStreamProject,
        protocol: Protocol,
    ): RuntimeType

    /** Render a builder for the given shape */
    fun renderBuilderForShape(
        writer: RustWriter,
        codegenContext: C,
        shape: StructureShape,
    )
}

object EventStreamTestTools {
    fun <C : CodegenContext> runTestCase(
        testCase: EventStreamTestModels.TestCase,
        requirements: EventStreamTestRequirements<C>,
        codegenTarget: CodegenTarget,
        variety: EventStreamTestVariety,
    ) {
        val model = EventStreamNormalizer.transform(OperationNormalizer.transform(testCase.model))
        val symbolProvider = requirements.createSymbolProvider(model)
        val serviceShape = model.expectShape(ShapeId.from("test#TestService")) as ServiceShape
        val codegenContext = requirements.createCodegenContext(
            model,
            symbolProvider,
            serviceShape,
            ShapeId.from(testCase.protocolShapeId),
            codegenTarget,
        )
        val test = generateTestProject(requirements, codegenContext, codegenTarget)
        val protocol = testCase.protocolBuilder(codegenContext)
        val generator = requirements.renderGenerator(codegenContext, test, protocol)

        test.project.lib {
            when (variety) {
                EventStreamTestVariety.Marshall -> writeMarshallTestCases(testCase, generator)
                EventStreamTestVariety.Unmarshall -> writeUnmarshallTestCases(testCase, codegenTarget, generator)
            }
        }
        test.project.compileAndTest()
    }

    private fun <C : CodegenContext> generateTestProject(
        requirements: EventStreamTestRequirements<C>,
        codegenContext: C,
        codegenTarget: CodegenTarget,
    ): TestEventStreamProject {
        val model = codegenContext.model
        val symbolProvider = codegenContext.symbolProvider
        val operationShape = model.expectShape(ShapeId.from("test#TestStreamOp")) as OperationShape
        val unionShape = model.expectShape(ShapeId.from("test#TestStream")) as UnionShape

        val project = TestWorkspace.testProject(symbolProvider)
        val operationSymbol = symbolProvider.toSymbol(operationShape)
        project.withModule(ErrorsModule) {
            val errors = model.shapes()
                .filter { shape -> shape.isStructureShape && shape.hasTrait<ErrorTrait>() }
                .map { it.asStructureShape().get() }
                .toList()
            when (codegenTarget) {
                CodegenTarget.CLIENT -> CombinedErrorGenerator(model, symbolProvider, operationSymbol, errors).render(this)
                CodegenTarget.SERVER -> ServerCombinedErrorGenerator(model, symbolProvider, operationSymbol, errors).render(this)
            }
            for (shape in model.shapes().filter { shape -> shape.isStructureShape && shape.hasTrait<ErrorTrait>() }) {
                StructureGenerator(model, symbolProvider, this, shape as StructureShape).render(codegenTarget)
                requirements.renderBuilderForShape(this, codegenContext, shape)
            }
        }
        project.withModule(ModelsModule) {
            val inputOutput = model.lookup<StructureShape>("test#TestStreamInputOutput")
            recursivelyGenerateModels(model, symbolProvider, inputOutput, this, codegenTarget)
        }
        project.withModule(RustModule.Output) {
            operationShape.outputShape(model).renderWithModelBuilder(model, symbolProvider, this)
        }
        return TestEventStreamProject(
            model,
            codegenContext.serviceShape,
            operationShape,
            unionShape,
            symbolProvider,
            project,
        )
    }

    private fun recursivelyGenerateModels(
        model: Model,
        symbolProvider: RustSymbolProvider,
        shape: Shape,
        writer: RustWriter,
        mode: CodegenTarget,
    ) {
        for (member in shape.members()) {
            val target = model.expectShape(member.target)
            if (target is StructureShape || target is UnionShape) {
                if (target is StructureShape) {
                    target.renderWithModelBuilder(model, symbolProvider, writer)
                } else if (target is UnionShape) {
                    UnionGenerator(model, symbolProvider, writer, target, renderUnknownVariant = mode.renderUnknownVariant()).render()
                }
                recursivelyGenerateModels(model, symbolProvider, target, writer, mode)
            }
        }
    }
}
