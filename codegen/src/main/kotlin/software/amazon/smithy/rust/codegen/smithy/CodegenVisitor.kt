/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ServiceGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolLoader
import software.amazon.smithy.rust.codegen.smithy.traits.SyntheticInputTrait
import software.amazon.smithy.rust.codegen.smithy.transformers.AddErrorMessage
import software.amazon.smithy.rust.codegen.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.smithy.transformers.RemoveEventStreamOperations
import software.amazon.smithy.rust.codegen.util.CommandFailed
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasTrait
import software.amazon.smithy.rust.codegen.util.runCommand
import java.util.logging.Logger

class CodegenVisitor(context: PluginContext, private val codegenDecorator: RustCodegenDecorator) :
    ShapeVisitor.Default<Unit>() {

    private val logger = Logger.getLogger(javaClass.name)
    private val settings = RustSettings.from(context.model, context.settings)

    private val symbolProvider: RustSymbolProvider
    private val rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    private val model: Model
    private val codegenContext: CodegenContext
    private val protocolGenerator: ProtocolGeneratorFactory<ProtocolGenerator>
    private val httpGenerator: ProtocolGenerator

    init {
        val symbolVisitorConfig =
            SymbolVisitorConfig(runtimeConfig = settings.runtimeConfig, codegenConfig = settings.codegenConfig)
        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        val (protocol, generator) = ProtocolLoader(
            codegenDecorator.protocols(service.id, ProtocolLoader.DefaultProtocols)
        ).protocolFor(context.model, service)
        protocolGenerator = generator
        model = generator.transformModel(codegenDecorator.transformModel(service, baseModel))
        val baseProvider = RustCodegenPlugin.baseSymbolProvider(model, service, symbolVisitorConfig)
        symbolProvider = codegenDecorator.symbolProvider(generator.symbolProvider(model, baseProvider))

        codegenContext =
            CodegenContext(model, symbolProvider, settings.runtimeConfig, service, protocol, settings.moduleName)
        rustCrate = RustCrate(
            context.fileManifest,
            symbolProvider,
            DefaultPublicModules
        )
        httpGenerator = protocolGenerator.buildProtocolGenerator(codegenContext)
    }

    private fun baselineTransform(model: Model) =
        model.let(RecursiveShapeBoxer::transform)
            .letIf(settings.codegenConfig.addMessageToErrors, AddErrorMessage::transform)
            .let(OperationNormalizer::transform)
            .let { RemoveEventStreamOperations.transform(it, settings) }
            .let(EventStreamNormalizer::transform)

    fun execute() {
        logger.info("generating Rust client...")
        val service = settings.getService(model)
        val serviceShapes = Walker(model).walkShapes(service)
        serviceShapes.forEach { it.accept(this) }
        codegenDecorator.extras(codegenContext, rustCrate)
        rustCrate.finalize(
            settings,
            codegenDecorator.libRsCustomizations(
                codegenContext,
                listOf()
            )
        )
        try {
            "cargo fmt".runCommand(fileManifest.baseDir, timeout = settings.codegenConfig.formatTimeoutSeconds.toLong())
        } catch (err: CommandFailed) {
            logger.warning("Failed to run cargo fmt: [${service.id}]\n${err.output}")
        }

        logger.info("Rust Client generation complete!")
    }

    override fun getDefault(shape: Shape?) {
    }

    override fun structureShape(shape: StructureShape) {
        logger.fine("generating a structure...")
        rustCrate.useShapeWriter(shape) { writer ->
            StructureGenerator(model, symbolProvider, writer, shape).render()
            if (!shape.hasTrait<SyntheticInputTrait>()) {
                val builderGenerator = BuilderGenerator(codegenContext.model, codegenContext.symbolProvider, shape)
                builderGenerator.render(writer)
                writer.implBlock(shape, symbolProvider) {
                    builderGenerator.renderConvenienceMethod(this)
                }
            }
        }
    }

    override fun stringShape(shape: StringShape) {
        shape.getTrait<EnumTrait>()?.also { enum ->
            rustCrate.useShapeWriter(shape) { writer ->
                EnumGenerator(model, symbolProvider, writer, shape, enum).render()
            }
        }
    }

    override fun unionShape(shape: UnionShape) {
        rustCrate.useShapeWriter(shape) {
            UnionGenerator(model, symbolProvider, it, shape).render()
        }
    }

    override fun serviceShape(shape: ServiceShape) {
        ServiceGenerator(
            rustCrate,
            httpGenerator,
            protocolGenerator.support(),
            codegenContext,
            codegenDecorator
        ).render()
    }
}
