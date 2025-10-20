/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ClientEnumGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.CustomizableOperationImplGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.ErrorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.error.OperationErrorGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolTestGenerator
import software.amazon.smithy.rust.codegen.client.smithy.protocols.ClientProtocolLoader
import software.amazon.smithy.rust.codegen.client.smithy.transformers.AddErrorMessage
import software.amazon.smithy.rust.codegen.client.smithy.transformers.DisableStalledStreamProtection
import software.amazon.smithy.rust.codegen.core.rustlang.EscapeFor
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustReservedWords
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.contextName
import software.amazon.smithy.rust.codegen.core.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.util.CommandError
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.core.util.serviceNameOrDefault
import software.amazon.smithy.rust.codegen.core.util.toSnakeCase
import java.util.logging.Logger

/**
 * Entry point for client code generation
 */
class ClientCodegenVisitor(
    context: PluginContext,
    private val codegenDecorator: ClientCodegenDecorator,
) : ShapeVisitor.Default<Unit>() {
    private val logger = Logger.getLogger(javaClass.name)
    private val settings = ClientRustSettings.from(context.model, context.settings)

    private val symbolProvider: RustSymbolProvider
    private val rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    private val model: Model
    private var codegenContext: ClientCodegenContext
    private val protocolGeneratorFactory: ProtocolGeneratorFactory<OperationGenerator, ClientCodegenContext>
    private val operationGenerator: OperationGenerator

    init {
        val rustSymbolProviderConfig =
            RustSymbolProviderConfig(
                runtimeConfig = settings.runtimeConfig,
                renameExceptions = settings.codegenConfig.renameExceptions,
                nullabilityCheckMode = settings.codegenConfig.nullabilityCheckMode,
                moduleProvider = ClientModuleProvider,
                nameBuilderFor = { symbol -> "${symbol.name}Builder" },
            )

        val baseModel = baselineTransform(context.model)
        val untransformedService = settings.getService(baseModel)
        val (protocol, generator) =
            ClientProtocolLoader(
                codegenDecorator.protocols(untransformedService.id, ClientProtocolLoader.DefaultProtocols),
            ).protocolFor(context.model, untransformedService)
        protocolGeneratorFactory = generator
        model = codegenDecorator.transformModel(untransformedService, baseModel, settings)
        // the model transformer _might_ change the service shape
        val service = settings.getService(model)
        symbolProvider =
            RustClientCodegenPlugin.baseSymbolProvider(
                settings,
                model,
                service,
                rustSymbolProviderConfig,
                codegenDecorator,
            )

        codegenContext =
            ClientCodegenContext(
                model,
                symbolProvider,
                null,
                service,
                protocol,
                settings,
                codegenDecorator,
            )

        codegenContext =
            codegenContext.copy(
                moduleDocProvider =
                    codegenDecorator.moduleDocumentationCustomization(
                        codegenContext,
                        ClientModuleDocProvider(codegenContext, service.serviceNameOrDefault("the service")),
                    ),
                protocolImpl = protocolGeneratorFactory.protocol(codegenContext),
            )

        rustCrate =
            RustCrate(
                context.fileManifest,
                symbolProvider,
                codegenContext.settings.codegenConfig,
                codegenContext.expectModuleDocProvider(),
            )
        operationGenerator = protocolGeneratorFactory.buildProtocolGenerator(codegenContext)
    }

    /**
     * Base model transformation applied to all services
     * See below for details.
     */
    internal fun baselineTransform(model: Model) =
        model
            // Flattens mixins out of the model and removes them from the model
            .let { ModelTransformer.create().flattenAndRemoveMixins(it) }
            // Add errors attached at the service level to the models
            .let { ModelTransformer.create().copyServiceErrorsToOperations(it, settings.getService(it)) }
            // Add `Box<T>` to recursive shapes as necessary
            .let(RecursiveShapeBoxer()::transform)
            // Normalize the `message` field on errors when enabled in settings (default: true)
            .letIf(settings.codegenConfig.addMessageToErrors, AddErrorMessage::transform)
            // NormalizeOperations by ensuring every operation has an input & output shape
            .let(OperationNormalizer::transform)
            // Normalize event stream operations
            .let(EventStreamNormalizer::transform)
            // Mark operations incompatible with stalled stream protection as such
            .let(DisableStalledStreamProtection::transformModel)

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
        val serviceShapes = DirectedWalker(model).walkShapes(service)
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
                listOf(),
            ),
            protocolId = codegenContext.protocol,
        )
        try {
            // use an increased max_width to make rustfmt fail less frequently
            "cargo fmt -- --config max_width=150".runCommand(
                fileManifest.baseDir,
                timeout = settings.codegenConfig.formatTimeoutSeconds.toLong(),
            )
        } catch (err: CommandError) {
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
        ServiceGenerator(rustCrate, codegenContext, codegenDecorator).render()
    }

    override fun getDefault(shape: Shape?) {
    }

    private fun privateModule(shape: Shape): RustModule.LeafModule =
        RustModule.private(privateModuleName(shape), parent = symbolProvider.moduleForShape(shape))

    private fun privateModuleName(shape: Shape): String =
        shape.contextName(codegenContext.serviceShape).let(this::privateModuleName)

    private fun privateModuleName(name: String): String =
        // Add the underscore to avoid colliding with public module names
        "_" + RustReservedWords.escapeIfNeeded(name.toSnakeCase(), EscapeFor.ModuleName)

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
        val (renderStruct, renderBuilder) =
            when (val errorTrait = shape.getTrait<ErrorTrait>()) {
                null -> {
                    val struct: Writable = {
                        StructureGenerator(
                            model,
                            symbolProvider,
                            this,
                            shape,
                            codegenDecorator.structureCustomizations(codegenContext, emptyList()),
                            structSettings = codegenContext.structSettings(),
                        ).render()

                        implBlock(symbolProvider.toSymbol(shape)) {
                            BuilderGenerator.renderConvenienceMethod(this, symbolProvider, shape)
                            if (codegenContext.protocolImpl?.httpBindingResolver?.handlesEventStreamInitialResponse(
                                    shape,
                                ) == true
                            ) {
                                BuilderGenerator.renderIntoBuilderMethod(this, symbolProvider, shape)
                            }
                        }
                    }
                    val builder: Writable = {
                        BuilderGenerator(
                            codegenContext.model,
                            codegenContext.symbolProvider,
                            shape,
                            codegenDecorator.builderCustomizations(codegenContext, emptyList()),
                        ).render(this)
                    }
                    struct to builder
                }

                else -> {
                    val errorGenerator =
                        ErrorGenerator(
                            model,
                            symbolProvider,
                            shape,
                            errorTrait,
                            codegenDecorator.errorImplCustomizations(codegenContext, emptyList()),
                            codegenContext.structSettings(),
                        )
                    errorGenerator::renderStruct to errorGenerator::renderBuilder
                }
            }

        val privateModule = privateModule(shape)
        rustCrate.inPrivateModuleWithReexport(privateModule, symbolProvider.toSymbol(shape)) {
            renderStruct(this)
        }
        rustCrate.inPrivateModuleWithReexport(privateModule, symbolProvider.symbolForBuilder(shape)) {
            renderBuilder(this)
        }
    }

    /**
     * String Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually `EnumTrait` applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        if (shape.hasTrait<EnumTrait>()) {
            val privateModule = privateModule(shape)
            rustCrate.inPrivateModuleWithReexport(privateModule, symbolProvider.toSymbol(shape)) {
                ClientEnumGenerator(
                    codegenContext,
                    shape,
                    codegenDecorator.enumCustomizations(codegenContext, emptyList()),
                ).render(this)
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
        rustCrate.inPrivateModuleWithReexport(privateModule(shape), symbolProvider.toSymbol(shape)) {
            UnionGenerator(model, symbolProvider, this, shape, renderUnknownVariant = true).render()
        }
        if (shape.isEventStream()) {
            rustCrate.withModule(symbolProvider.moduleForEventStreamError(shape)) {
                OperationErrorGenerator(
                    model,
                    symbolProvider,
                    shape,
                    codegenDecorator.errorCustomizations(codegenContext, emptyList()),
                ).render(this)
            }
        }
    }

    /**
     * Generate operations
     */
    override fun operationShape(operationShape: OperationShape) {
        rustCrate.useShapeWriter(operationShape) {
            // Render the operation shape
            operationGenerator.renderOperation(
                this,
                operationShape,
                codegenDecorator,
            )

            // render protocol tests into `operation.rs` (note operationWriter vs. inputWriter)
            codegenDecorator.protocolTestGenerator(
                codegenContext,
                ClientProtocolTestGenerator(
                    codegenContext,
                    protocolGeneratorFactory.support(),
                    operationShape,
                ),
            ).render(this)
        }

        rustCrate.withModule(symbolProvider.moduleForOperationError(operationShape)) {
            OperationErrorGenerator(
                model,
                symbolProvider,
                operationShape,
                codegenDecorator.errorCustomizations(codegenContext, emptyList()),
            ).render(this)
        }

        rustCrate.withModule(ClientRustModule.Client.customize) {
            CustomizableOperationImplGenerator(
                codegenContext,
                operationShape,
                codegenDecorator.operationCustomizations(codegenContext, operationShape, emptyList()),
            ).render(this)
        }
    }
}
