/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.BlobShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.HttpVersion
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplGenerator
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.isEventStream
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.ConstrainedPythonBlobGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonApplicationGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerEventStreamErrorGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerEventStreamWrapperGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerOperationErrorGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerOperationHandlerGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerStructureGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerUnionGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.protocols.PythonServerProtocolLoader
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleDocProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.ServerSymbolProviders
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.createInlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.UnconstrainedUnionGenerator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.traits.isReachableFromOperationInput
import software.amazon.smithy.rust.codegen.server.smithy.withModuleOrWithStructureBuilderModule

/**
 * Entrypoint for Python server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 *
 * This class inherits from [ServerCodegenVisitor] since it uses most of the functionalities of the super class
 * and have to override the symbol provider with [PythonServerSymbolProvider].
 */
class PythonServerCodegenVisitor(
    context: PluginContext,
    private val codegenDecorator: ServerCodegenDecorator,
) : ServerCodegenVisitor(context, codegenDecorator) {
    init {
        // Python server bindings are only compatible with the legacy HTTP 0.x server.
        // Force HTTP 0.x regardless of configuration settings.
        // `publicConstrainedTypes` must always be `false` for the Python server, since Python generates its own
        // wrapper newtypes.
        settings =
            settings.copy(
                runtimeConfig = settings.runtimeConfig.copy(httpVersion = HttpVersion.Http0x),
                codegenConfig =
                    settings.codegenConfig.copy(
                        publicConstrainedTypes = false,
                        http1x = false,
                    ),
            )

        val rustSymbolProviderConfig =
            RustSymbolProviderConfig(
                runtimeConfig = settings.runtimeConfig,
                renameExceptions = false,
                nullabilityCheckMode = NullableIndex.CheckMode.SERVER,
                moduleProvider = ServerModuleProvider,
            )
        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        val (protocol, generator) =
            PythonServerProtocolLoader(
                codegenDecorator.protocols(
                    service.id,
                    PythonServerProtocolLoader.defaultProtocols(settings.runtimeConfig),
                ),
            )
                .protocolFor(context.model, service)
        protocolGeneratorFactory = generator

        model = codegenDecorator.transformModel(service, baseModel, settings)

        fun baseSymbolProviderFactory(
            settings: ServerRustSettings,
            model: Model,
            serviceShape: ServiceShape,
            rustSymbolProviderConfig: RustSymbolProviderConfig,
            publicConstrainedTypes: Boolean,
            includeConstraintShapeProvider: Boolean,
            codegenDecorator: ServerCodegenDecorator,
        ) =
            RustServerCodegenPythonPlugin.baseSymbolProvider(settings, model, serviceShape, rustSymbolProviderConfig, publicConstrainedTypes, includeConstraintShapeProvider, codegenDecorator)

        val serverSymbolProviders =
            ServerSymbolProviders.from(
                settings,
                model,
                service,
                rustSymbolProviderConfig,
                settings.codegenConfig.publicConstrainedTypes,
                codegenDecorator,
                ::baseSymbolProviderFactory,
            )

        // Override `codegenContext` which carries the various symbol providers.
        val moduleDocProvider =
            codegenDecorator.moduleDocumentationCustomization(
                codegenContext,
                PythonServerModuleDocProvider(ServerModuleDocProvider(codegenContext)),
            )
        codegenContext =
            ServerCodegenContext(
                model,
                serverSymbolProviders.symbolProvider,
                moduleDocProvider,
                service,
                protocol,
                settings,
                serverSymbolProviders.unconstrainedShapeSymbolProvider,
                serverSymbolProviders.constrainedShapeSymbolProvider,
                serverSymbolProviders.constraintViolationSymbolProvider,
                serverSymbolProviders.pubCrateConstrainedShapeSymbolProvider,
            )

        // Override `rustCrate` which carries the symbolProvider.
        rustCrate =
            RustCrate(
                context.fileManifest,
                codegenContext.symbolProvider,
                settings.codegenConfig,
                codegenContext.expectModuleDocProvider(),
            )
        // Override `protocolGenerator` which carries the symbolProvider.
        protocolGenerator = protocolGeneratorFactory.buildProtocolGenerator(codegenContext)
    }

    /**
     * Structure Shape Visitor
     *
     * For each structure shape, generate:
     * - A Rust structure for the shape ([StructureGenerator]).
     * - `pyo3::PyClass` trait implementation.
     * - A builder for the shape.
     *
     * This function _does not_ generate any serializers.
     */
    override fun structureShape(shape: StructureShape) {
        logger.info("[python-server-codegen] Generating a structure $shape")
        rustCrate.useShapeWriter(shape) {
            // Use Python specific structure generator that adds the #[pyclass] attribute
            // and #[pymethods] implementation.
            PythonServerStructureGenerator(model, codegenContext, this, shape).render()

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

    /**
     * String Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually [EnumTrait] applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        fun pythonServerEnumGeneratorFactory(
            codegenContext: ServerCodegenContext,
            shape: StringShape,
        ) = PythonServerEnumGenerator(codegenContext, shape, validationExceptionConversionGenerator)
        stringShape(shape, ::pythonServerEnumGeneratorFactory)
    }

    /**
     * Union Shape Visitor
     *
     * Generate an `enum` for union shapes.
     *
     * Note: this does not generate serializers
     */
    override fun unionShape(shape: UnionShape) {
        logger.info("[python-server-codegen] Generating an union shape $shape")
        rustCrate.useShapeWriter(shape) {
            PythonServerUnionGenerator(model, codegenContext, this, shape, renderUnknownVariant = false).render()
        }

        if (shape.isReachableFromOperationInput() &&
            shape.canReachConstrainedShape(
                model,
                codegenContext.symbolProvider,
            )
        ) {
            logger.info("[python-server-codegen] Generating an unconstrained type for union shape $shape")
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
                PythonServerEventStreamErrorGenerator(model, codegenContext.symbolProvider, shape).render(this)
            }
        }
    }

    override fun protocolTestsForOperation(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        logger.warning("[python-server-codegen] Protocol tests are disabled for this language")
    }

    /**
     * Generate service-specific code for the model:
     * - Serializers
     * - Deserializers
     * - Trait implementations
     * - Protocol tests
     * - Operation structures
     * - Python operation handlers
     */
    override fun serviceShape(shape: ServiceShape) {
        super.serviceShape(shape)

        logger.info("[python-server-codegen] Generating a service $shape")

        val serverProtocol = protocolGeneratorFactory.protocol(codegenContext) as ServerProtocol
        rustCrate.withModule(PythonServerRustModule.PythonServerApplication) {
            PythonApplicationGenerator(codegenContext, serverProtocol)
                .render(this)
        }
    }

    override fun operationShape(shape: OperationShape) {
        super.operationShape(shape)

        rustCrate.withModule(PythonServerRustModule.PythonOperationAdapter) {
            PythonServerOperationHandlerGenerator(codegenContext, shape).render(this)
        }

        rustCrate.withModule(ServerRustModule.Error) {
            PythonServerOperationErrorGenerator(codegenContext.model, codegenContext.symbolProvider, shape).render(this)
        }
    }

    override fun memberShape(shape: MemberShape) {
        super.memberShape(shape)

        if (shape.isEventStream(model)) {
            rustCrate.withModule(PythonServerRustModule.PythonEventStream) {
                PythonServerEventStreamWrapperGenerator(codegenContext, shape).render(this)
            }
        }
    }

    override fun blobShape(shape: BlobShape) {
        logger.info("[python-server-codegen] Generating a service $shape")
        super.blobShape(shape)

        if (shape.isDirectlyConstrained(codegenContext.symbolProvider)) {
            rustCrate.withModuleOrWithStructureBuilderModule(ServerRustModule.Model, shape, codegenContext) {
                ConstrainedPythonBlobGenerator(
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
