/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.ByteShape
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.IntegerShape
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.model.shapes.LongShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.NumberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.SetShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeVisitor
import software.amazon.smithy.model.shapes.ShortShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.model.traits.LengthTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.implBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.CoreRustSettings
import software.amazon.smithy.rust.codegen.core.smithy.DirectedWalker
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.StructureGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplGenerator
import software.amazon.smithy.rust.codegen.core.smithy.generators.lifetimeDeclaration
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolGeneratorFactory
import software.amazon.smithy.rust.codegen.core.smithy.transformers.EventStreamNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.OperationNormalizer
import software.amazon.smithy.rust.codegen.core.smithy.transformers.RecursiveShapeBoxer
import software.amazon.smithy.rust.codegen.core.util.CommandError
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasEventStreamMember
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.core.util.runCommand
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionConstraintViolationGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.CollectionTraitInfo
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedBlobGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedNumberGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedStringGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ConstrainedTraitForEnumGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.MapConstraintViolationGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.PubCrateConstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.PubCrateConstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ScopeMacroGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerBuilderGeneratorWithoutPublicConstrainedTypes
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerOperationErrorGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerOperationGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerRootGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerRuntimeTypesReExportsGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerServiceGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServerStructureConstrainedTraitImpl
import software.amazon.smithy.rust.codegen.server.smithy.generators.ServiceConfigGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedCollectionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedMapGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedUnionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.ValidationExceptionConversionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.isBuilderFallible
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolTestGenerator
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.transformers.AttachValidationExceptionToConstrainedOperationInputs
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ConstrainedMemberTransform
import software.amazon.smithy.rust.codegen.server.smithy.transformers.RecursiveConstraintViolationBoxer
import software.amazon.smithy.rust.codegen.server.smithy.transformers.RemoveEbsModelValidationException
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ServerProtocolBasedTransformationFactory
import software.amazon.smithy.rust.codegen.server.smithy.transformers.ShapesReachableFromOperationInputTagger
import java.util.logging.Logger

/**
 * Entrypoint for server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 */
open class ServerCodegenVisitor(
    context: PluginContext,
    private val codegenDecorator: ServerCodegenDecorator,
) : ShapeVisitor.Default<Unit>() {
    protected val logger = Logger.getLogger(javaClass.name)
    protected var settings = ServerRustSettings.from(context.model, context.settings)

    protected var rustCrate: RustCrate
    private val fileManifest = context.fileManifest
    protected var model: Model
    protected var codegenContext: ServerCodegenContext
    protected var protocolGeneratorFactory: ProtocolGeneratorFactory<ServerProtocolGenerator, ServerCodegenContext>
    protected var protocolGenerator: ServerProtocolGenerator
    protected var validationExceptionConversionGenerator: ValidationExceptionConversionGenerator

    init {
        val rustSymbolProviderConfig =
            RustSymbolProviderConfig(
                runtimeConfig = settings.runtimeConfig,
                renameExceptions = false,
                nullabilityCheckMode = NullableIndex.CheckMode.SERVER,
                moduleProvider = ServerModuleProvider,
            )

        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        model = codegenDecorator.transformModel(service, baseModel, settings)
        val serverSymbolProviders =
            ServerSymbolProviders.from(
                settings,
                model,
                service,
                rustSymbolProviderConfig,
                settings.codegenConfig.publicConstrainedTypes,
                codegenDecorator,
                RustServerCodegenPlugin::baseSymbolProvider,
            )
        val (protocolShape, protocolGeneratorFactory) =
            ServerProtocolLoader(
                codegenDecorator.protocols(
                    service.id,
                    ServerProtocolLoader.DefaultProtocols,
                ),
            )
                .protocolFor(context.model, service)
        codegenContext =
            ServerCodegenContext(
                model,
                serverSymbolProviders.symbolProvider,
                null,
                service,
                protocolShape,
                settings,
                serverSymbolProviders.unconstrainedShapeSymbolProvider,
                serverSymbolProviders.constrainedShapeSymbolProvider,
                serverSymbolProviders.constraintViolationSymbolProvider,
                serverSymbolProviders.pubCrateConstrainedShapeSymbolProvider,
            )
        this.protocolGeneratorFactory = protocolGeneratorFactory

        // We can use a not-null assertion because [CombinedServerCodegenDecorator] returns a not null value.
        validationExceptionConversionGenerator = codegenDecorator.validationExceptionConversion(codegenContext)!!

        codegenContext =
            codegenContext.copy(
                moduleDocProvider =
                    codegenDecorator.moduleDocumentationCustomization(
                        codegenContext,
                        ServerModuleDocProvider(codegenContext),
                    ),
            )

        rustCrate =
            RustCrate(
                context.fileManifest,
                codegenContext.symbolProvider,
                settings.codegenConfig,
                codegenContext.expectModuleDocProvider(),
            )
        protocolGenerator = this.protocolGeneratorFactory.buildProtocolGenerator(codegenContext)
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
            .let(RecursiveShapeBoxer()::transform)
            // Add `Box<T>` to recursive constraint violations as necessary
            .let(RecursiveConstraintViolationBoxer::transform)
            // Normalize operations by adding synthetic input and output shapes to every operation
            .let(OperationNormalizer::transform)
            // Transforms constrained member shapes into non-constrained member shapes targeting a new shape that
            // has the member's constraints.
            .let(ConstrainedMemberTransform::transform)
            // Remove the EBS model's own `ValidationException`, which collides with `smithy.framework#ValidationException`
            .let(RemoveEbsModelValidationException::transform)
            // Attach the `smithy.framework#ValidationException` error to operations whose inputs are constrained,
            // if either the operation belongs to a service in the allowlist, or the codegen flag to add the exception has been set.
            .let { AttachValidationExceptionToConstrainedOperationInputs.transform(it, settings) }
            // Tag aggregate shapes reachable from operation input
            .let(ShapesReachableFromOperationInputTagger::transform)
            // Remove traits that are not supported by the chosen protocol.
            .let { ServerProtocolBasedTransformationFactory.transform(it, settings) }
            // Normalize event stream operations
            .let(EventStreamNormalizer::transform)

    /**
     * Exposure purely for unit test purposes.
     */
    internal fun baselineTransformInternalTest(model: Model) = baselineTransform(model)

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

        val validationExceptionShapeId = validationExceptionConversionGenerator.shapeId
        for (validationResult in listOf(
            validateModelHasAtMostOneValidationException(model, service),
            codegenDecorator.postprocessValidationExceptionNotAttachedErrorMessage(
                validateOperationsWithConstrainedInputHaveValidationExceptionAttached(
                    model,
                    service,
                    validationExceptionShapeId,
                ),
            ),
            validateUnsupportedConstraints(model, service, codegenContext.settings.codegenConfig),
            codegenDecorator.postprocessMultipleValidationExceptionsErrorMessage(
                validateOperationsWithConstrainedInputHaveOneValidationExceptionAttached(
                    model,
                    service,
                    validationExceptionShapeId,
                ),
            ),
        )) {
            for (logMessage in validationResult.messages) {
                // TODO(https://github.com/smithy-lang/smithy-rs/issues/1756): These are getting duplicated.
                logger.log(logMessage.level, logMessage.message)
            }
            if (validationResult.shouldAbort) {
                throw CodegenException(
                    "Unsupported constraints feature used; see error messages above for resolution",
                    validationResult,
                )
            }
        }

        rustCrate.initializeInlineModuleWriter(codegenContext.settings.codegenConfig.debugMode)

        val serviceShapes = DirectedWalker(model).walkShapes(service)
        serviceShapes.forEach { it.accept(this) }
        codegenDecorator.extras(codegenContext, rustCrate)

        rustCrate.getInlineModuleWriter().render()

        rustCrate.finalize(
            settings,
            model,
            codegenDecorator.crateManifestCustomizations(codegenContext),
            codegenDecorator.libRsCustomizations(codegenContext, listOf()),
            // TODO(https://github.com/smithy-lang/smithy-rs/issues/1287): Remove once the server codegen is far enough along.
            requireDocs = false,
            protocolId = codegenContext.protocol,
        )
        try {
            "cargo fmt".runCommand(
                fileManifest.baseDir,
                timeout = settings.codegenConfig.formatTimeoutSeconds.toLong(),
            )
        } catch (err: CommandError) {
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
            StructureGenerator(
                model,
                codegenContext.symbolProvider,
                this,
                shape,
                codegenDecorator.structureCustomizations(codegenContext, emptyList()),
                structSettings = codegenContext.structSettings(),
            ).render()

            shape.getTrait<ErrorTrait>()?.also { errorTrait ->
                ErrorImplGenerator(
                    model,
                    codegenContext.symbolProvider,
                    this,
                    shape,
                    errorTrait,
                    codegenDecorator.errorImplCustomizations(codegenContext, emptyList()),
                ).render(CodegenTarget.SERVER)
            }

            renderStructureShapeBuilder(shape, this)
        }
    }

    protected fun renderStructureShapeBuilder(
        shape: StructureShape,
        writer: RustWriter,
    ) {
        if (codegenContext.settings.codegenConfig.publicConstrainedTypes || shape.isReachableFromOperationInput()) {
            val serverBuilderGenerator =
                ServerBuilderGenerator(
                    codegenContext,
                    shape,
                    validationExceptionConversionGenerator,
                    protocolGenerator.protocol,
                )
            serverBuilderGenerator.render(rustCrate, writer)

            if (codegenContext.settings.codegenConfig.publicConstrainedTypes) {
                val lifetimes = shape.lifetimeDeclaration(codegenContext.symbolProvider)
                writer.rustBlock("impl $lifetimes ${codegenContext.symbolProvider.toSymbol(shape).name} $lifetimes") {
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
                ServerBuilderGeneratorWithoutPublicConstrainedTypes(
                    codegenContext,
                    shape,
                    validationExceptionConversionGenerator,
                    protocolGenerator.protocol,
                )
            serverBuilderGeneratorWithoutPublicConstrainedTypes.render(rustCrate, writer)

            writer.implBlock(codegenContext.symbolProvider.toSymbol(shape)) {
                serverBuilderGeneratorWithoutPublicConstrainedTypes.renderConvenienceMethod(this)
            }
        }
    }

    override fun listShape(shape: ListShape) = collectionShape(shape)

    override fun setShape(shape: SetShape) = collectionShape(shape)

    private fun collectionShape(shape: CollectionShape) {
        val renderUnconstrainedList =
            shape.isReachableFromOperationInput() &&
                shape.canReachConstrainedShape(
                    model,
                    codegenContext.symbolProvider,
                )
        val isDirectlyConstrained = shape.isDirectlyConstrained(codegenContext.symbolProvider)

        if (renderUnconstrainedList) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for collection shape $shape")
            rustCrate.withModuleOrWithStructureBuilderModule(
                ServerRustModule.UnconstrainedModule,
                shape,
                codegenContext,
            ) {
                UnconstrainedCollectionGenerator(
                    codegenContext,
                    rustCrate.createInlineModuleCreator(),
                    shape,
                ).render()
            }

            if (!isDirectlyConstrained) {
                logger.info("[rust-server-codegen] Generating a constrained type for collection shape $shape")
                rustCrate.withModuleOrWithStructureBuilderModule(
                    ServerRustModule.ConstrainedModule,
                    shape,
                    codegenContext,
                ) {
                    PubCrateConstrainedCollectionGenerator(
                        codegenContext,
                        rustCrate.createInlineModuleCreator(),
                        shape,
                    ).render()
                }
            }
        }

        val constraintsInfo = CollectionTraitInfo.fromShape(shape, codegenContext.constrainedShapeSymbolProvider)
        if (isDirectlyConstrained) {
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
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
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
                CollectionConstraintViolationGenerator(
                    codegenContext,
                    rustCrate.createInlineModuleCreator(),
                    shape, constraintsInfo,
                    validationExceptionConversionGenerator,
                ).render()
            }
        }
    }

    override fun mapShape(shape: MapShape) {
        val renderUnconstrainedMap =
            shape.isReachableFromOperationInput() &&
                shape.canReachConstrainedShape(
                    model,
                    codegenContext.symbolProvider,
                )
        val isDirectlyConstrained = shape.isDirectlyConstrained(codegenContext.symbolProvider)

        if (renderUnconstrainedMap) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for map $shape")
            rustCrate.withModuleOrWithStructureBuilderModule(
                ServerRustModule.UnconstrainedModule,
                shape,
                codegenContext,
            ) {
                UnconstrainedMapGenerator(
                    codegenContext,
                    rustCrate.createInlineModuleCreator(),
                    shape,
                ).render()
            }

            if (!isDirectlyConstrained) {
                logger.info("[rust-server-codegen] Generating a constrained type for map $shape")
                rustCrate.withModuleOrWithStructureBuilderModule(
                    ServerRustModule.ConstrainedModule,
                    shape,
                    codegenContext,
                ) {
                    PubCrateConstrainedMapGenerator(
                        codegenContext,
                        rustCrate.createInlineModuleCreator(),
                        shape,
                    ).render()
                }
            }
        }

        if (isDirectlyConstrained) {
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
                ConstrainedMapGenerator(
                    codegenContext,
                    this,
                    shape,
                    if (renderUnconstrainedMap) codegenContext.unconstrainedShapeSymbolProvider.toSymbol(shape) else null,
                ).render()
            }
        }

        if (isDirectlyConstrained || renderUnconstrainedMap) {
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
                MapConstraintViolationGenerator(
                    codegenContext,
                    rustCrate.createInlineModuleCreator(),
                    shape,
                    validationExceptionConversionGenerator,
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
        fun serverEnumGeneratorFactory(
            codegenContext: ServerCodegenContext,
            shape: StringShape,
        ) = ServerEnumGenerator(
            codegenContext,
            shape,
            validationExceptionConversionGenerator,
            codegenDecorator.enumCustomizations(codegenContext, emptyList()),
        )
        stringShape(shape, ::serverEnumGeneratorFactory)
    }

    override fun integerShape(shape: IntegerShape) = integralShape(shape)

    override fun shortShape(shape: ShortShape) = integralShape(shape)

    override fun longShape(shape: LongShape) = integralShape(shape)

    override fun byteShape(shape: ByteShape) = integralShape(shape)

    private fun integralShape(shape: NumberShape) {
        if (shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            logger.info("[rust-server-codegen] Generating a constrained integral $shape")
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
                ConstrainedNumberGenerator(
                    codegenContext, rustCrate.createInlineModuleCreator(),
                    this,
                    shape,
                    validationExceptionConversionGenerator,
                ).render()
            }
        }
    }

    protected fun stringShape(
        shape: StringShape,
        enumShapeGeneratorFactory: (codegenContext: ServerCodegenContext, shape: StringShape) -> EnumGenerator,
    ) {
        if (shape.hasTrait<EnumTrait>()) {
            logger.info("[rust-server-codegen] Generating an enum $shape")
            rustCrate.useShapeWriterOrUseWithStructureBuilder(shape, codegenContext) {
                enumShapeGeneratorFactory(codegenContext, shape).render(this)
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
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
                ConstrainedStringGenerator(
                    codegenContext,
                    rustCrate.createInlineModuleCreator(),
                    this,
                    shape,
                    validationExceptionConversionGenerator,
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
            UnionGenerator(model, codegenContext.symbolProvider, this, shape, renderUnknownVariant = false).render()
        }

        if (shape.isReachableFromOperationInput() &&
            shape.canReachConstrainedShape(
                model,
                codegenContext.symbolProvider,
            )
        ) {
            logger.info("[rust-server-codegen] Generating an unconstrained type for union shape $shape")
            rustCrate.withModule(ServerRustModule.UnconstrainedModule) modelsModuleWriter@{
                UnconstrainedUnionGenerator(
                    codegenContext,
                    rustCrate.createInlineModuleCreator(),
                    this@modelsModuleWriter,
                    shape,
                    validationExceptionConversionGenerator,
                ).render()
            }
        }

        if (shape.isEventStream()) {
            rustCrate.withModule(ServerRustModule.Error) {
                ServerOperationErrorGenerator(model, codegenContext.symbolProvider, shape).render(this)
            }
        }
    }

    /**
     * Generate protocol tests. This method can be overridden by other languages such as Python.
     */
    open fun protocolTestsForOperation(
        writer: RustWriter,
        shape: OperationShape,
    ) {
        codegenDecorator.protocolTestGenerator(
            codegenContext,
            ServerProtocolTestGenerator(
                codegenContext,
                protocolGeneratorFactory.support(),
                shape,
            ),
        ).render(writer)
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
        val serverProtocol = protocolGeneratorFactory.protocol(codegenContext) as ServerProtocol

        val configMethods = codegenDecorator.configMethods(codegenContext)
        val isConfigBuilderFallible = configMethods.isBuilderFallible()

        // Generate root.
        rustCrate.lib {
            ServerRootGenerator(
                serverProtocol,
                codegenContext,
                isConfigBuilderFallible,
            ).render(this)
        }

        // Generate server re-exports.
        rustCrate.withModule(ServerRustModule.Server) {
            ServerRuntimeTypesReExportsGenerator(codegenContext).render(this)
        }

        // Generate service module.
        rustCrate.withModule(ServerRustModule.Service) {
            ServerServiceGenerator(
                codegenContext,
                serverProtocol,
                isConfigBuilderFallible,
            ).render(this)

            ServiceConfigGenerator(codegenContext, configMethods).render(this)

            ScopeMacroGenerator(codegenContext).render(this)
        }

        codegenDecorator.postprocessServiceGenerateAdditionalStructures(shape)
            .forEach { structureShape -> this.structureShape(structureShape) }
    }

    /**
     * For each operation shape generate:
     *  - Operations ser/de
     *  - Errors via `ServerOperationErrorGenerator`
     *  - OperationShapes via `ServerOperationGenerator`
     *  - Additional structure shapes via `postprocessGenerateAdditionalStructures`
     */
    override fun operationShape(shape: OperationShape) {
        // Generate errors.
        rustCrate.withModule(ServerRustModule.Error) {
            ServerOperationErrorGenerator(model, codegenContext.symbolProvider, shape).render(this)
        }

        // Generate operation shapes.
        rustCrate.withModule(ServerRustModule.OperationShape) {
            ServerOperationGenerator(shape, codegenContext).render(this)
        }

        // Generate operations ser/de.
        rustCrate.withModule(ServerRustModule.Operation) {
            protocolGenerator.renderOperation(this, shape)
        }

        codegenDecorator.postprocessOperationGenerateAdditionalStructures(shape)
            .forEach { structureShape -> this.structureShape(structureShape) }

        // Generate protocol tests.
        rustCrate.withModule(ServerRustModule.Operation) {
            protocolTestsForOperation(this, shape)
        }
    }

    override fun blobShape(shape: BlobShape) {
        logger.info("[rust-server-codegen] Generating a service $shape")
        if (shape.hasEventStreamMember(model)) {
            return super.blobShape(shape)
        }

        if (shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
                ConstrainedBlobGenerator(
                    codegenContext,
                    rustCrate.createInlineModuleCreator(),
                    this,
                    shape,
                    validationExceptionConversionGenerator,
                ).render()
            }
        }
    }
}
