
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerEnumGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerServiceGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerStructureGenerator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader
import software.amazon.smithy.rust.codegen.smithy.DefaultPublicModules
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.util.getTrait

/**
 * Entrypoint for Python server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 *
 * This class inherits from [ServerCodegenVisitor] since it uses most of the functionalities of the super class
 * and have to override the symbol provider with [PythonServerSymbolProvider].
 */
class PythonServerCodegenVisitor(
    context: PluginContext,
    codegenDecorator: RustCodegenDecorator<ServerCodegenContext>
) : ServerCodegenVisitor(context, codegenDecorator) {

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
        val baseProvider = PythonCodegenServerPlugin.baseSymbolProvider(model, service, symbolVisitorConfig)
        // Override symbolProvider.
        symbolProvider =
            codegenDecorator.symbolProvider(generator.symbolProvider(model, baseProvider))

        // Override `codegenContext` which carries the symbolProvider.
        codegenContext = ServerCodegenContext(model, symbolProvider, service, protocol, settings)

        // Override `rustCrate` which carries the symbolProvider.
        rustCrate = RustCrate(context.fileManifest, symbolProvider, DefaultPublicModules, settings.codegenConfig)
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
        rustCrate.useShapeWriter(shape) { writer ->
            // Use Python specific structure generator that adds the #[pyclass] attribute
            // and #[pymethods] implementation.
            PythonServerStructureGenerator(model, symbolProvider, writer, shape).render(CodegenTarget.SERVER)
            val builderGenerator =
                BuilderGenerator(codegenContext.model, codegenContext.symbolProvider, shape)
            builderGenerator.render(writer)
            writer.implBlock(shape, symbolProvider) {
                builderGenerator.renderConvenienceMethod(this)
            }
        }
    }

    /**
     * String Shape Visitor
     *
     * Although raw strings require no code generation, enums are actually [EnumTrait] applied to string shapes.
     */
    override fun stringShape(shape: StringShape) {
        logger.info("[rust-server-codegen] Generating an enum $shape")
        shape.getTrait<EnumTrait>()?.also { enum ->
            rustCrate.useShapeWriter(shape) { writer ->
                PythonServerEnumGenerator(model, symbolProvider, writer, shape, enum, codegenContext.runtimeConfig).render()
            }
        }
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
            protocolGeneratorFactory.protocol(codegenContext).httpBindingResolver,
            codegenContext,
        )
            .render()
    }
}
