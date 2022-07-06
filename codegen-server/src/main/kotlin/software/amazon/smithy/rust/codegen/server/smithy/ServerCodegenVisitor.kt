/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedStringGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedTraitForEnumGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.MapConstraintViolationGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.PubCrateConstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.PubCrateConstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerServiceGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerStructureConstrainedTraitImpl
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedUnionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader
import software.amazon.smithy.rust.codegen.smithy.Constrained
import software.amazon.smithy.rust.codegen.smithy.DefaultPublicModules
import software.amazon.smithy.rust.codegen.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.Unconstrained
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.smithy.generators.protocol.ProtocolGenerator
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolGeneratorFactory
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
 * Entrypoint for server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 */
open class ServerCodegenVisitor(
    context: PluginContext,
    private val codegenDecorator: RustCodegenDecorator<ServerCodegenContext>
) : ShapeVisitor.Default<Unit>() {

    protected val logger = Logger.getLogger(javaClass.name)
    protected val settings = ServerRustSettings.from(context.model, context.settings)

    private val symbolProvider: RustSymbolProvider
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider
    private val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider
    private val constraintViolationSymbolProvider: ConstraintViolationSymbolProvider
    private val rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    private val model: Model
    private val codegenContext: ServerCodegenContext
    private val protocolGeneratorFactory: ProtocolGeneratorFactory<ProtocolGenerator, ServerCodegenContext>
    private val protocolGenerator: ProtocolGenerator
    private val unconstrainedModule =
        RustModule.private(Unconstrained.namespace, "Unconstrained types for constrained shapes.")
    private val constrainedModule =
        RustModule.private(Constrained.namespace, "Constrained types for constrained shapes.")
    private val shapesReachableFromOperationInputs: Set<Shape>

    init {
        val symbolVisitorConfig =
            SymbolVisitorConfig(
                runtimeConfig = settings.runtimeConfig,
                renameExceptions = false,
                handleRequired = true,
                handleRustBoxing = true,
            )
        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        val (protocol, generator) =
            ServerProtocolLoader(
                codegenDecorator.protocols(
                    service.id,
                    ServerProtocolLoader.DefaultProtocols
                )
            )
                .protocolFor(context.model, service)
        protocolGeneratorFactory = generator
        model = generator.transformModel(codegenDecorator.transformModel(service, baseModel))
        val baseProvider = RustCodegenServerPlugin.baseSymbolProvider(model, service, symbolVisitorConfig)
        // TODO Separate commit: No `CodegenDecorator` is altering the symbol provider, so we might as well remove the `symbolProvider`
        //  method from the `RustCodegenDecorator` interface.
        symbolProvider =
            codegenDecorator.symbolProvider(generator.symbolProvider(model, baseProvider))
        unconstrainedShapeSymbolProvider = UnconstrainedShapeSymbolProvider(
            codegenDecorator.symbolProvider(
                generator.symbolProvider(
                    model, RustCodegenServerPlugin.baseSymbolProvider(
                        model,
                        service,
                        symbolVisitorConfig,
                        publicConstrainedTypesEnabled = false
                    )
                )
            ), model, service
        )
        pubCrateConstrainedShapeSymbolProvider = PubCrateConstrainedShapeSymbolProvider(symbolProvider, model, service)
        constraintViolationSymbolProvider = ConstraintViolationSymbolProvider(symbolProvider, model, service)

        codegenContext = ServerCodegenContext(
            model,
            symbolProvider,
            service,
            protocol,
            settings,
            unconstrainedShapeSymbolProvider = unconstrainedShapeSymbolProvider
        )

        rustCrate = RustCrate(context.fileManifest, symbolProvider, DefaultPublicModules, settings.codegenConfig)
        protocolGenerator = protocolGeneratorFactory.buildProtocolGenerator(codegenContext)

        val walker = Walker(model)
        val inputShapes = model.operationShapes.map { model.expectShape(it.inputShape, StructureShape::class.java) }
        shapesReachableFromOperationInputs = inputShapes.flatMap { walker.walkShapes(it) }.toSet()
    }

    /**
     * Base model transformation applied to all services.
     * See below for details.
     */
    protected fun baselineTransform(model: Model) =
        model
            // Add errors attached at the service level to the models
            .let { ModelTransformer.create().copyServiceErrorsToOperations(it, settings.getService(it)) }
            // Add `Box<T>` to recursive shapes as necessary
            .let(RecursiveShapeBoxer::transform)
            // Normalize operations by adding synthetic input and output shapes to every operation
            .let(OperationNormalizer::transform)
            // Drop unsupported event stream operations from the model
            .let { RemoveEventStreamOperations.transform(it, settings) }
            // Normalize event stream operations
            .let(EventStreamNormalizer::transform)

    /**
     * Execute code generation
     *
     * 1. Load the service from [CoreRustSettings].
     * 2. Traverse every shape in the closure of the service.
     * 3. Loop through each shape and visit them (calling the override functions in this class)
     * 4. Call finalization tasks specified by decorators.
     * 5. Write the in-memory buffers out to files.
     *
     * The main work of code generation (serializers, protocols, etc.) is handled in `fn serviceShape` below.
     */
    fun execute() {
        val service = settings.getService(model)
        logger.info(
            "[rust-server-codegen] Generating Rust server for service $service, protocol ${codegenContext.protocol}"
        )
        val serviceShapes = Walker(model).walkShapes(service)
        serviceShapes.forEach { it.accept(this) }
        codegenDecorator.extras(codegenContext, rustCrate)
        rustCrate.finalize(
            settings,
            model,
            codegenDecorator.crateManifestCustomizations(codegenContext),
            codegenDecorator.libRsCustomizations(codegenContext, listOf()),
            // TODO(https://github.com/awslabs/smithy-rs/issues/1287): Remove once the server codegen is far enough along.
            requireDocs = false
        )
        try {
            "cargo fmt".runCommand(
                fileManifest.baseDir,
                timeout = settings.codegenConfig.formatTimeoutSeconds.toLong()
            )
        } catch (err: CommandFailed) {
            logger.warning(
                "[rust-server-codegen] Failed to run cargo fmt: [${service.id}]\n${err.output}"
            )
        }
        logger.info("[rust-server-codegen] Rust server generation complete!")
    }

    override fun getDefault(shape: Shape?) {}

    /**
     * Structure Shape Visitor
     *
     * For each structure shape, generate:
     * - A Rust structure for the shape ([StructureGenerator]).
     * - A builder for the shape.
     *
     * This function _does not_ generate any serializers.
     */
    override fun structureShape(shape: StructureShape) {
        logger.info("[rust-server-codegen] Generating a structure $shape")
        rustCrate.useShapeWriter(shape) { writer ->
            StructureGenerator(model, symbolProvider, writer, shape).render(CodegenTarget.SERVER)
            val builderGenerator = ServerBuilderGenerator(
                codegenContext,
                shape,
                if (shapesReachableFromOperationInputs.contains(shape)) pubCrateConstrainedShapeSymbolProvider else null
            )
            builderGenerator.render(writer)
            writer.implBlock(shape, symbolProvider) {
                builderGenerator.renderConvenienceMethod(this)
            }

            if (shapesReachableFromOperationInputs.contains(shape)) {
                ServerStructureConstrainedTraitImpl(symbolProvider, shape, writer).render()
            }
        }
    }

    override fun listShape(shape: ListShape) = collectionShape(shape)
    override fun setShape(shape: SetShape) = collectionShape(shape)

    private fun collectionShape(shape: CollectionShape) {
        if (shapesReachableFromOperationInputs.contains(shape) && shape.canReachConstrainedShape(
                model,
                symbolProvider
            )
        ) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for collection shape $shape")
            rustCrate.withModule(unconstrainedModule) { unconstrainedModuleWriter ->
                rustCrate.withModule(ModelsModule) { modelsModuleWriter ->
                    UnconstrainedCollectionGenerator(
                        model,
                        symbolProvider,
                        unconstrainedShapeSymbolProvider,
                        pubCrateConstrainedShapeSymbolProvider,
                        constraintViolationSymbolProvider,
                        unconstrainedModuleWriter,
                        modelsModuleWriter,
                        shape
                    ).render()
                }
            }

            logger.info("[rust-server-codegen] Generating a constrained type for collection shape $shape")
            rustCrate.withModule(constrainedModule) { writer ->
                PubCrateConstrainedCollectionGenerator(
                    model,
                    symbolProvider,
                    unconstrainedShapeSymbolProvider,
                    pubCrateConstrainedShapeSymbolProvider,
                    writer,
                    shape
                ).render()
            }
        }
    }

    override fun mapShape(shape: MapShape) {
        val renderUnconstrainedMap =
            shapesReachableFromOperationInputs.contains(shape) && shape.canReachConstrainedShape(
                model,
                symbolProvider
            )
        if (renderUnconstrainedMap) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for map $shape")
            rustCrate.withModule(unconstrainedModule) { unconstrainedModuleWriter ->
                UnconstrainedMapGenerator(
                    model,
                    symbolProvider,
                    unconstrainedShapeSymbolProvider,
                    pubCrateConstrainedShapeSymbolProvider,
                    constraintViolationSymbolProvider,
                    unconstrainedModuleWriter,
                    shape
                ).render()
            }

            if (!shape.isDirectlyConstrained(symbolProvider)) {
                logger.info("[rust-server-codegen] Generating a constrained type for map $shape")
                rustCrate.withModule(constrainedModule) { writer ->
                    PubCrateConstrainedMapGenerator(
                        model,
                        symbolProvider,
                        unconstrainedShapeSymbolProvider,
                        pubCrateConstrainedShapeSymbolProvider,
                        writer,
                        shape
                    ).render()
                }
            }
        }

        val isDirectlyConstrained = shape.isDirectlyConstrained(symbolProvider)
        if (isDirectlyConstrained) {
            rustCrate.useShapeWriter(shape) { writer ->
                ConstrainedMapGenerator(
                    model,
                    symbolProvider,
                    constraintViolationSymbolProvider,
                    writer,
                    shape,
                    if (renderUnconstrainedMap) unconstrainedShapeSymbolProvider.toSymbol(shape) else null
                ).render()
            }
        }

        if (isDirectlyConstrained || renderUnconstrainedMap) {
            rustCrate.withModule(ModelsModule) { modelsModuleWriter ->
                MapConstraintViolationGenerator(
                    model,
                    symbolProvider,
                    constraintViolationSymbolProvider,
                    modelsModuleWriter,
                    shape
                ).render()
            }
        }
    }

    /**
     * Enum Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually [EnumTrait] applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        shape.getTrait<EnumTrait>()?.also { enum ->
            logger.info("[rust-server-codegen] Generating an enum $shape")
            rustCrate.useShapeWriter(shape) { writer ->
                ServerEnumGenerator(
                    model,
                    symbolProvider,
                    constraintViolationSymbolProvider,
                    writer,
                    shape,
                    enum
                ).render()
                ConstrainedTraitForEnumGenerator(
                    model,
                    symbolProvider,
                    writer,
                    shape
                ).render()
            }
        }

        if (shape.hasTrait<EnumTrait>() && shape.isDirectlyConstrained(symbolProvider)) {
            logger.warning(
                """
                String shape $shape has an `enum` trait and another constraint trait. This is valid according to the Smithy
                spec v1 IDL, but it's unclear what the semantics are. In any case, the Smithy CLI should enforce the
                constraints (which it currently does not), not each code generator.
                See https://github.com/awslabs/smithy/issues/1121f for more information.
                """.trimIndent()
            )
        } else if (shape.isDirectlyConstrained(symbolProvider)) {
            logger.info("[rust-server-codegen] Generating a constrained string $shape")
            rustCrate.withModule(ModelsModule) { writer ->
                ConstrainedStringGenerator(
                    model,
                    symbolProvider,
                    constraintViolationSymbolProvider,
                    writer,
                    shape
                ).render()
            }
        }
    }

    /**
     * Union Shape Visitor
     *
     * Generate an `enum` for union shapes.
     *
     * This function _does not_ generate any serializers.
     */
    override fun unionShape(shape: UnionShape) {
        logger.info("[rust-server-codegen] Generating an union shape $shape")
        rustCrate.useShapeWriter(shape) {
            UnionGenerator(model, symbolProvider, it, shape, renderUnknownVariant = false).render()
        }

        if (shapesReachableFromOperationInputs.contains(shape) && shape.canReachConstrainedShape(
                model,
                symbolProvider
            )
        ) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for union shape $shape")
            rustCrate.withModule(unconstrainedModule) { unconstrainedModuleWriter ->
                rustCrate.withModule(ModelsModule) { modelsModuleWriter ->
                    UnconstrainedUnionGenerator(
                        model,
                        symbolProvider,
                        unconstrainedShapeSymbolProvider,
                        pubCrateConstrainedShapeSymbolProvider,
                        constraintViolationSymbolProvider,
                        unconstrainedModuleWriter,
                        modelsModuleWriter,
                        shape
                    ).render()
                }
            }
        }
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
        logger.info("[rust-server-codegen] Generating a service $shape")
        ServerServiceGenerator(
            rustCrate,
            protocolGenerator,
            protocolGeneratorFactory.support(),
            protocolGeneratorFactory.protocol(codegenContext).httpBindingResolver,
            codegenContext,
        )
            .render()
    }
}
