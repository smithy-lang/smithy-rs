/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
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
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.ServiceGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
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

/**
 * Base Entrypoint for Code generation
 */
class CodegenVisitor(context: PluginContext, private val codegenDecorator: RustCodegenDecorator<ClientCodegenContext>) :
    ShapeVisitor.Default<Unit>() {

    private val logger = Logger.getLogger(javaClass.name)
    private val settings = ClientRustSettings.from(context.model, context.settings)

    private val symbolProvider: RustSymbolProvider
    private val rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    private val model: Model
    private val codegenContext: ClientCodegenContext
    private val protocolGeneratorFactory: ProtocolGeneratorFactory<ProtocolGenerator, ClientCodegenContext>
    private val protocolGenerator: ProtocolGenerator

    init {
        val symbolVisitorConfig =
            SymbolVisitorConfig(
                runtimeConfig = settings.runtimeConfig,
                renameExceptions = settings.codegenConfig.renameExceptions,
                handleRequired = false,
                handleRustBoxing = true,
            )
        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        val (protocol, generator) = ProtocolLoader(
            codegenDecorator.protocols(service.id, ProtocolLoader.DefaultProtocols)
        ).protocolFor(context.model, service)
        protocolGeneratorFactory = generator
        model = generator.transformModel(codegenDecorator.transformModel(service, baseModel))
        val baseProvider = RustCodegenPlugin.baseSymbolProvider(model, service, symbolVisitorConfig)
        symbolProvider = codegenDecorator.symbolProvider(generator.symbolProvider(model, baseProvider))

        codegenContext = ClientCodegenContext(model, symbolProvider, service, protocol, settings)
        rustCrate = RustCrate(
            context.fileManifest,
            symbolProvider,
            DefaultPublicModules,
            codegenContext.settings.codegenConfig
        )
        protocolGenerator = protocolGeneratorFactory.buildProtocolGenerator(codegenContext)
    }

    /**
     * Base model transformation applied to all services
     * See below for details.
     */
    private fun baselineTransform(model: Model) =
        model
            // Add errors attached at the service level to the models
            .let { ModelTransformer.create().copyServiceErrorsToOperations(it, settings.getService(it)) }
            // Add `Box<T>` to recursive shapes as necessary
            .let(RecursiveShapeBoxer::transform)
            // Normalize the `message` field on errors when enabled in settings (default: true)
            .letIf(settings.codegenConfig.addMessageToErrors, AddErrorMessage::transform)
            // NormalizeOperations by ensuring every operation has an input & output shape
            .let(OperationNormalizer::transform)
            // Drop unsupported event stream operations from the model
            .let { RemoveEventStreamOperations.transform(it, settings) }
            // - Normalize event stream operations
            .let(EventStreamNormalizer::transform)

    /**
     * Execute code generation
     *
     * 1. Load the service from RustSettings
     * 2. Traverse every shape in the closure of the service.
     * 3. Loop through each shape and visit them (calling the override functions in this class)
     * 4. Call finalization tasks specified by decorators.
     * 5. Write the in-memory buffers out to files.
     *
     * The main work of code generation (serializers, protocols, etc.) is handled in `fn serviceShape` below.
     */
    fun execute() {
        logger.info("generating Rust client...")
        val service = settings.getService(model)
        val serviceShapes = Walker(model).walkShapes(service)
        serviceShapes.forEach { it.accept(this) }
        codegenDecorator.extras(codegenContext, rustCrate)
        // finalize actually writes files into the base directory, renders any inline functions that were used, and
        // performs finalization like generating a Cargo.toml
        rustCrate.finalize(
            settings,
            model,
            codegenDecorator.crateManifestCustomizations(codegenContext),
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

    /**
     * Generate service-specific code for the model:
     * - Serializers
     * - Deserializers
     * - Fluent client
     * - Trait implementations
     * - Protocol tests
     * - Operation structures
     */
    override fun serviceShape(shape: ServiceShape) {
        ServiceGenerator(
            rustCrate,
            protocolGenerator,
            protocolGeneratorFactory.support(),
            codegenContext,
            codegenDecorator
        ).render()
    }

    override fun getDefault(shape: Shape?) {
    }

    /**
     * Structure Shape Visitor
     *
     * For each structure shape, generate:
     * - A Rust structure for the shape (StructureGenerator)
     * - A builder for the shape
     *
     * This function _does not_ generate any serializers
     */
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

    /**
     * String Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually `EnumTrait` applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        shape.getTrait<EnumTrait>()?.also { enum ->
            rustCrate.useShapeWriter(shape) { writer ->
                EnumGenerator(model, symbolProvider, writer, shape, enum).render()
            }
        }
    }

    /**
     * Union Shape Visitor
     *
     * Generate an `enum` for union shapes.
     *
     * Note: this does not generate serializers
     */
    override fun unionShape(shape: UnionShape) {
        rustCrate.useShapeWriter(shape) {
            UnionGenerator(model, symbolProvider, it, shape, renderUnknownVariant = true).render()
        }
    }
}
