
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

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
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.smithy.CodegenTarget
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerOperationHandlerGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerServiceGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerStructureGenerator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.smithy.ServerSymbolProviders
import software.amazon.smithy.rust.codegen.server.smithy.customize.ServerCodegenDecorator
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocol
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader

/**
 * Entrypoint for Python server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 *
 * This class inherits from [ServerCodegenVisitor] since it uses most of the functionalities of the super class
 * and have to override the symbol provider with [PythonServerSymbolProvider].
 */
class PythonServerCodegenVisitor(
    context: PluginContext,
    codegenDecorator: ServerCodegenDecorator,
) : ServerCodegenVisitor(context, codegenDecorator) {

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

        // `publicConstrainedTypes` must always be `false` for the Python server, since Python generates its own
        // wrapper newtypes.
        settings = settings.copy(codegenConfig = settings.codegenConfig.copy(publicConstrainedTypes = false))

        fun baseSymbolProviderFactory(
            model: Model,
            serviceShape: ServiceShape,
            symbolVisitorConfig: SymbolVisitorConfig,
            publicConstrainedTypes: Boolean,
        ) = PythonCodegenServerPlugin.baseSymbolProvider(model, serviceShape, symbolVisitorConfig, publicConstrainedTypes)

        val serverSymbolProviders = ServerSymbolProviders.from(
            model,
            service,
            symbolVisitorConfig,
            settings.codegenConfig.publicConstrainedTypes,
            ::baseSymbolProviderFactory,
        )

        // Override `codegenContext` which carries the various symbol providers.
        codegenContext =
            ServerCodegenContext(
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

        // Override `rustCrate` which carries the symbolProvider.
        rustCrate = RustCrate(context.fileManifest, codegenContext.symbolProvider, settings.codegenConfig)
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
            PythonServerStructureGenerator(model, codegenContext.symbolProvider, this, shape).render(CodegenTarget.SERVER)

            renderStructureShapeBuilder(shape, this)
        }
    }

    /**
     * String Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually [EnumTrait] applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        fun pythonServerEnumGeneratorFactory(codegenContext: ServerCodegenContext, writer: RustWriter, shape: StringShape) =
            PythonServerEnumGenerator(codegenContext, writer, shape)
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
        throw CodegenException("Union shapes are not supported in Python yet")
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
        logger.info("[python-server-codegen] Generating a service $shape")
        PythonServerServiceGenerator(
            rustCrate,
            protocolGenerator,
            protocolGeneratorFactory.support(),
            protocolGeneratorFactory.protocol(codegenContext) as ServerProtocol,
            codegenContext,
        )
            .render()
    }

    override fun operationShape(shape: OperationShape) {
        super.operationShape(shape)
        rustCrate.withModule(RustModule.public("python_operation_adaptor")) {
            PythonServerOperationHandlerGenerator(codegenContext, shape).render(this)
        }
    }
}
