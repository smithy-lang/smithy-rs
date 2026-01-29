/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.typescript.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.codegen.core.CodegenException
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.NullableIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.model.traits.ErrorTrait
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.RustSymbolProviderConfig
import software.amazon.smithy.rust.codegen.core.smithy.generators.error.ErrorImplGenerator
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleDocProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerModuleProvider
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustModule
import software.amazon.smithy.rust.codegen.server.smithy.ServerRustSettings
import software.amazon.smithy.rust.codegen.server.smithy.ServerSymbolProviders
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader
import software.amazon.smithy.rust.codegen.server.typescript.smithy.generators.TsApplicationGenerator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.generators.TsServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.generators.TsServerOperationErrorGenerator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.generators.TsServerOperationHandlerGenerator
import software.amazon.smithy.rust.codegen.server.typescript.smithy.generators.TsServerStructureGenerator

/**
 * Entrypoint for Typescript server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 *
 * This class inherits from [ServerCodegenVisitor] since it uses most of the functionalities of the super class
 * and have to override the symbol provider with [TsServerSymbolProvider].
 */
class TsServerCodegenVisitor(
    context: PluginContext,
    private val codegenDecorator: ServerCodegenDecorator,
) : ServerCodegenVisitor(context, codegenDecorator) {
    init {
        val symbolVisitorConfig =
            RustSymbolProviderConfig(
                runtimeConfig = settings.runtimeConfig,
                renameExceptions = false,
                nullabilityCheckMode = NullableIndex.CheckMode.SERVER,
                moduleProvider = ServerModuleProvider,
            )
        val baseModel = baselineTransform(context.model)
        val service = settings.getService(baseModel)
        val (protocol, generator) =
            ServerProtocolLoader(
                codegenDecorator.protocols(
                    service.id,
                    ServerProtocolLoader.defaultProtocols(),
                ),
            )
                .protocolFor(context.model, service)
        protocolGeneratorFactory = generator

        model = codegenDecorator.transformModel(service, baseModel, settings)

        // `publicConstrainedTypes` must always be `false` for the Typescript server, since Typescript generates its own
        // wrapper newtypes.
        settings = settings.copy(codegenConfig = settings.codegenConfig.copy(publicConstrainedTypes = false))

        fun baseSymbolProviderFactory(
            settings: ServerRustSettings,
            model: Model,
            serviceShape: ServiceShape,
            rustSymbolProviderConfig: RustSymbolProviderConfig,
            publicConstrainedTypes: Boolean,
            includeConstraintShapeProvider: Boolean,
            codegenDecorator: ServerCodegenDecorator,
        ) = RustServerCodegenTsPlugin.baseSymbolProvider(
            settings,
            model,
            serviceShape,
            rustSymbolProviderConfig,
            publicConstrainedTypes,
            includeConstraintShapeProvider,
            codegenDecorator,
        )

        val serverSymbolProviders =
            ServerSymbolProviders.from(
                settings,
                model,
                service,
                symbolVisitorConfig,
                settings.codegenConfig.publicConstrainedTypes,
                codegenDecorator,
                ::baseSymbolProviderFactory,
            )

        // Override `codegenContext` which carries the various symbol providers.
        codegenContext =
            ServerCodegenContext(
                model,
                serverSymbolProviders.symbolProvider,
                null,
                service,
                protocol,
                settings,
                serverSymbolProviders.unconstrainedShapeSymbolProvider,
                serverSymbolProviders.constrainedShapeSymbolProvider,
                serverSymbolProviders.constraintViolationSymbolProvider,
                serverSymbolProviders.pubCrateConstrainedShapeSymbolProvider,
            )

        codegenContext =
            codegenContext.copy(
                moduleDocProvider =
                    codegenDecorator.moduleDocumentationCustomization(
                        codegenContext,
                        TsServerModuleDocProvider(ServerModuleDocProvider(codegenContext)),
                    ),
            )

        // Override `rustCrate` which carries the symbolProvider.
        rustCrate =
            RustCrate(
                context.fileManifest, codegenContext.symbolProvider, settings.codegenConfig,
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
     * - A builder for the shape.
     *
     * This function _does not_ generate any serializers.
     */
    override fun structureShape(shape: StructureShape) {
        logger.info("[js-server-codegen] Generating a structure $shape")
        rustCrate.useShapeWriter(shape) {
            // Use Typescript specific structure generator that adds the #[napi] attribute
            // and implementation.
            TsServerStructureGenerator(model, codegenContext.symbolProvider, this, shape).render()

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
        fun tsServerEnumGeneratorFactory(
            codegenContext: ServerCodegenContext,
            shape: StringShape,
        ) = TsServerEnumGenerator(codegenContext, shape, validationExceptionConversionGenerator, emptyList())
        stringShape(shape, ::tsServerEnumGeneratorFactory)
    }

    /**
     * Union Shape Visitor
     *
     * Generate an `enum` for union shapes.
     *
     * Note: this does not generate serializers
     */
    override fun unionShape(shape: UnionShape) {
        throw CodegenException("Union shapes are not supported in Typescript yet")
    }

    /**
     * Generate service-specific code for the model:
     * - Serializers
     * - Deserializers
     * - Trait implementations
     * - Protocol tests
     * - Operation structures
     * - Typescript operation handlers
     */
    override fun serviceShape(shape: ServiceShape) {
        super.serviceShape(shape)

        logger.info("[ts-server-codegen] Generating a service $shape")

        val serverProtocol = protocolGeneratorFactory.protocol(codegenContext) as ServerProtocol
        rustCrate.withModule(TsServerRustModule.TsServerApplication) {
            TsApplicationGenerator(codegenContext, serverProtocol).render(this)
        }
    }

    override fun operationShape(shape: OperationShape) {
        super.operationShape(shape)
        rustCrate.withModule(TsServerRustModule.TsOperationAdapter) {
            TsServerOperationHandlerGenerator(codegenContext, shape).render(this)
        }

        rustCrate.withModule(ServerRustModule.Error) {
            TsServerOperationErrorGenerator(codegenContext.model, codegenContext.symbolProvider, shape).render(this)
        }
    }
}
