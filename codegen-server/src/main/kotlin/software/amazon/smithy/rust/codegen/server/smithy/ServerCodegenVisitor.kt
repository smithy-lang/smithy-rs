/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.neighbor.Walker
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.ConstrainedModule
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.ModelsModule
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.core.smithy.UnconstrainedModule
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.util.CommandFailed
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionConstraintViolationGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedNumberGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedStringGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedTraitForEnumGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.MapConstraintViolationGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.PubCrateConstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.PubCrateConstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGeneratorWithoutPublicConstrainedTypes
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerServiceGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerStructureConstrainedTraitImpl
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedUnionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.transformers.AttachValidationExceptionToConstrainedOperationInputsInAllowList
import software.amazon.smithy.rust.codegen.server.smithy.transformers.RemoveEbsModelValidationException
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ShapesReachableFromOperationInputTagger
import java.util.logging.Logger

/**
 * Entrypoint for server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 */
open class ServerCodegenVisitor(
    context: PluginContext,
    private val codegenDecorator: RustCodegenDecorator<ServerProtocolGenerator, ServerCodegenContext>,
) : ShapeVisitor.Default<Unit>() {

    protected val logger = Logger.getLogger(javaClass.name)
    protected var settings = ServerRustSettings.from(context.model, context.settings)

    protected var rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    protected var model: Model
    protected var codegenContext: ServerCodegenContext
    protected var protocolGeneratorFactory: ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext>
    protected var protocolGenerator: ServerProtocolGenerator

    init {
        val symbolVisitorConfig =
            SymbolVisitorConfig(
                runtimeConfig = settings.runtimeConfig,
                renameExceptions = false,
                nullabilityCheckMode = NullableIndex.CheckMode.SERVER,
            )

        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        val (protocol, generator) =
            ServerProtocolLoader(
                codegenDecorator.protocols(
                    service.id,
                    ServerProtocolLoader.DefaultProtocols,
                ),
            )
                .protocolFor(context.model, service)
        protocolGeneratorFactory = generator

        model = codegenDecorator.transformModel(service, baseModel)

        val serverSymbolProviders = ServerSymbolProviders.from(
            model,
            service,
            symbolVisitorConfig,
            settings.codegenConfig.publicConstrainedTypes,
            RustCodegenServerPlugin::baseSymbolProvider,
        )

        codegenContext = ServerCodegenContext(
            model,
            serverSymbolProviders.symbolProvider,
            service,
            protocol,
            settings,
            serverSymbolProviders.unconstrainedShapeSymbolProvider,
            serverSymbolProviders.constrainedShapeSymbolProvider,
            serverSymbolProviders.constraintViolationSymbolProvider,
            serverSymbolProviders.pubCrateConstrainedShapeSymbolProvider,
        )

        rustCrate = RustCrate(context.fileManifest, codegenContext.symbolProvider, settings.codegenConfig)
        protocolGenerator = protocolGeneratorFactory.buildProtocolGenerator(codegenContext)
    }

    /**
     * Base model transformation applied to all services.
     * See below for details.
     */
    protected fun baselineTransform(model: Model) =
        model
            // Flattens mixins out of the model and removes them from the model
            .let { ModelTransformer.create().flattenAndRemoveMixins(it) }
            // Add errors attached at the service level to the models
            .let { ModelTransformer.create().copyServiceErrorsToOperations(it, settings.getService(it)) }
            // Add `Box<T>` to recursive shapes as necessary
            .let(RecursiveShapeBoxer::transform)
            // Normalize operations by adding synthetic input and output shapes to every operation
            .let(OperationNormalizer::transform)
            // Remove the EBS model's own `ValidationException`, which collides with `smithy.framework#ValidationException`
            .let(RemoveEbsModelValidationException::transform)
            // Attach the `smithy.framework#ValidationException` error to operations whose inputs are constrained,
            // if they belong to a service in an allowlist
            .let(AttachValidationExceptionToConstrainedOperationInputsInAllowList::transform)
            // Tag aggregate shapes reachable from operation input
            .let(ShapesReachableFromOperationInputTagger::transform)
            // Normalize event stream operations
            .let(EventStreamNormalizer::transform)

    /**
     * Exposure purely for unit test purposes.
     */
    internal fun baselineTransformInternalTest(model: Model) =
        baselineTransform(model)

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
        logger.warning(
            "[rust-server-codegen] Generating Rust server for service $service, protocol ${codegenContext.protocol}",
        )

        for (validationResult in listOf(
            validateOperationsWithConstrainedInputHaveValidationExceptionAttached(
                model,
                service,
            ),
            validateUnsupportedConstraints(model, service, codegenContext.settings.codegenConfig),
        )) {
            for (logMessage in validationResult.messages) {
                // TODO(https://github.com/awslabs/smithy-rs/issues/1756): These are getting duplicated.
                logger.log(logMessage.level, logMessage.message)
            }
            if (validationResult.shouldAbort) {
                throw CodegenException("Unsupported constraints feature used; see error messages above for resolution")
            }
        }

        val serviceShapes = Walker(model).walkShapes(service)
        serviceShapes.forEach { it.accept(this) }
        codegenDecorator.extras(codegenContext, rustCrate)
        rustCrate.finalize(
            settings,
            model,
            codegenDecorator.crateManifestCustomizations(codegenContext),
            codegenDecorator.libRsCustomizations(codegenContext, listOf()),
            // TODO(https://github.com/awslabs/smithy-rs/issues/1287): Remove once the server codegen is far enough along.
            requireDocs = false,
        )
        try {
            "cargo fmt".runCommand(
                fileManifest.baseDir,
                timeout = settings.codegenConfig.formatTimeoutSeconds.toLong(),
            )
        } catch (err: CommandFailed) {
            logger.info(
                "[rust-server-codegen] Failed to run cargo fmt: [${service.id}]\n${err.output}",
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
        rustCrate.useShapeWriter(shape) {
            StructureGenerator(model, codegenContext.symbolProvider, this, shape).render(CodegenTarget.SERVER)

            renderStructureShapeBuilder(shape, this)
        }
    }

    protected fun renderStructureShapeBuilder(
        shape: StructureShape,
        writer: RustWriter,
    ) {
        if (codegenContext.settings.codegenConfig.publicConstrainedTypes || shape.isReachableFromOperationInput()) {
            val serverBuilderGenerator = ServerBuilderGenerator(codegenContext, shape)
            serverBuilderGenerator.render(writer)

            if (codegenContext.settings.codegenConfig.publicConstrainedTypes) {
                writer.implBlock(shape, codegenContext.symbolProvider) {
                    serverBuilderGenerator.renderConvenienceMethod(this)
                }
            }
        }

        if (shape.isReachableFromOperationInput()) {
            ServerStructureConstrainedTraitImpl(
                codegenContext.symbolProvider,
                codegenContext.settings.codegenConfig.publicConstrainedTypes,
                shape,
                writer,
            ).render()
        }

        if (!codegenContext.settings.codegenConfig.publicConstrainedTypes) {
            val serverBuilderGeneratorWithoutPublicConstrainedTypes =
                ServerBuilderGeneratorWithoutPublicConstrainedTypes(codegenContext, shape)
            serverBuilderGeneratorWithoutPublicConstrainedTypes.render(writer)

            writer.implBlock(shape, codegenContext.symbolProvider) {
                serverBuilderGeneratorWithoutPublicConstrainedTypes.renderConvenienceMethod(this)
            }
        }
    }

    override fun listShape(shape: ListShape) = collectionShape(shape)
    override fun setShape(shape: SetShape) = collectionShape(shape)

    private fun collectionShape(shape: CollectionShape) {
        val renderUnconstrainedList =
            shape.isReachableFromOperationInput() && shape.canReachConstrainedShape(
                model,
                codegenContext.symbolProvider,
            )
        val isDirectlyConstrained = shape.isDirectlyConstrained(codegenContext.symbolProvider)

        if (renderUnconstrainedList) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for collection shape $shape")
            rustCrate.withModule(UnconstrainedModule) {
                UnconstrainedCollectionGenerator(
                    codegenContext,
                    this,
                    shape,
                ).render()
            }

            if (!isDirectlyConstrained) {
                logger.info("[rust-server-codegen] Generating a constrained type for collection shape $shape")
                rustCrate.withModule(ConstrainedModule) {
                    PubCrateConstrainedCollectionGenerator(codegenContext, this, shape).render()
                }
            }
        }

        val constraintsInfo = CollectionTraitInfo.fromShape(shape)
        if (isDirectlyConstrained) {
            rustCrate.withModule(ModelsModule) {
                ConstrainedCollectionGenerator(
                    codegenContext,
                    this,
                    shape,
                    constraintsInfo,
                    if (renderUnconstrainedList) codegenContext.unconstrainedShapeSymbolProvider.toSymbol(shape) else null,
                ).render()
            }
        }

        if (isDirectlyConstrained || renderUnconstrainedList) {
            rustCrate.withModule(ModelsModule) {
                CollectionConstraintViolationGenerator(codegenContext, this, shape, constraintsInfo).render()
            }
        }
    }

    override fun mapShape(shape: MapShape) {
        val renderUnconstrainedMap =
            shape.isReachableFromOperationInput() && shape.canReachConstrainedShape(
                model,
                codegenContext.symbolProvider,
            )
        val isDirectlyConstrained = shape.isDirectlyConstrained(codegenContext.symbolProvider)

        if (renderUnconstrainedMap) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for map $shape")
            rustCrate.withModule(UnconstrainedModule) {
                UnconstrainedMapGenerator(codegenContext, this, shape).render()
            }

            if (!isDirectlyConstrained) {
                logger.info("[rust-server-codegen] Generating a constrained type for map $shape")
                rustCrate.withModule(ConstrainedModule) {
                    PubCrateConstrainedMapGenerator(codegenContext, this, shape).render()
                }
            }
        }

        if (isDirectlyConstrained) {
            rustCrate.withModule(ModelsModule) {
                ConstrainedMapGenerator(
                    codegenContext,
                    this,
                    shape,
                    if (renderUnconstrainedMap) codegenContext.unconstrainedShapeSymbolProvider.toSymbol(shape) else null,
                ).render()
            }
        }

        if (isDirectlyConstrained || renderUnconstrainedMap) {
            rustCrate.withModule(ModelsModule) {
                MapConstraintViolationGenerator(codegenContext, this, shape).render()
            }
        }
    }

    /**
     * Enum Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually [EnumTrait] applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        fun serverEnumGeneratorFactory(codegenContext: ServerCodegenContext, writer: RustWriter, shape: StringShape) =
            ServerEnumGenerator(codegenContext, writer, shape)
        stringShape(shape, ::serverEnumGeneratorFactory)
    }

    override fun integerShape(shape: IntegerShape) {
        if (shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            logger.info("[rust-server-codegen] Generating a constrained integer $shape")
            rustCrate.withModule(ModelsModule) {
                ConstrainedNumberGenerator(codegenContext, this, shape).render()
            }
        }
    }

    override fun shortShape(shape: ShortShape) {
        if (shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            logger.info("[rust-server-codegen] Generating a constrained short $shape")
            rustCrate.withModule(ModelsModule) {
                ConstrainedNumberGenerator(codegenContext, this, shape).render()
            }
        }
    }

    override fun longShape(shape: LongShape) {
        if (shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            logger.info("[rust-server-codegen] Generating a constrained long $shape")
            rustCrate.withModule(ModelsModule) {
                ConstrainedNumberGenerator(codegenContext, this, shape).render()
            }
        }
    }

    override fun byteShape(shape: ByteShape) {
        if (shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            logger.info("[rust-server-codegen] Generating a constrained byte $shape")
            rustCrate.withModule(ModelsModule) {
                ConstrainedNumberGenerator(codegenContext, this, shape).render()
            }
        }
    }

    protected fun stringShape(
        shape: StringShape,
        enumShapeGeneratorFactory: (codegenContext: ServerCodegenContext, writer: RustWriter, shape: StringShape) -> ServerEnumGenerator,
    ) {
        if (shape.hasTrait<EnumTrait>()) {
            logger.info("[rust-server-codegen] Generating an enum $shape")
            rustCrate.useShapeWriter(shape) {
                enumShapeGeneratorFactory(codegenContext, this, shape).render()
                ConstrainedTraitForEnumGenerator(model, codegenContext.symbolProvider, this, shape).render()
            }
        }

        if (shape.hasTrait<EnumTrait>() && shape.hasTrait<LengthTrait>()) {
            logger.warning(
                """
                String shape $shape has an `enum` trait and the `length` trait. This is valid according to the Smithy
                IDL v1 spec, but it's unclear what the semantics are. In any case, the Smithy core libraries should enforce the
                constraints (which it currently does not), not each code generator.
                See https://github.com/awslabs/smithy/issues/1121f for more information.
                """.trimIndent().replace("\n", " "),
            )
        } else if (!shape.hasTrait<EnumTrait>() && shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            logger.info("[rust-server-codegen] Generating a constrained string $shape")
            rustCrate.withModule(ModelsModule) {
                ConstrainedStringGenerator(codegenContext, this, shape).render()
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
            UnionGenerator(model, codegenContext.symbolProvider, this, shape, renderUnknownVariant = false).render()
        }

        if (shape.isReachableFromOperationInput() && shape.canReachConstrainedShape(
                model,
                codegenContext.symbolProvider,
            )
        ) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for union shape $shape")
            rustCrate.withModule(UnconstrainedModule) unconstrainedModuleWriter@{
                rustCrate.withModule(ModelsModule) modelsModuleWriter@{
                    UnconstrainedUnionGenerator(
                        codegenContext,
                        this@unconstrainedModuleWriter,
                        this@modelsModuleWriter,
                        shape,
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
            protocolGeneratorFactory.protocol(codegenContext) as ServerProtocol,
            codegenContext,
        )
            .render()
    }
}
