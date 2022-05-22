
/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.python.smithy

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.shapes.UnionShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerServiceGenerator
import software.amazon.smithy.rust.codegen.server.python.smithy.generators.PythonServerStructureGenerator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenVisitor
import software.amazon.smithy.rust.codegen.server.smithy.protocols.ServerProtocolLoader
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.CodegenMode
import software.amazon.smithy.rust.codegen.smithy.DefaultPublicModules
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.SymbolVisitorConfig
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.BuilderGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.CodegenTarget
import software.amazon.smithy.rust.codegen.smithy.generators.EnumGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.UnionGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.implBlock
import software.amazon.smithy.rust.codegen.util.getTrait

/**
 * Entrypoint for server-side code generation. This class will walk the in-memory model and
 * generate all the needed types by calling the accept() function on the available shapes.
 */
class PythonServerCodegenVisitor(context: PluginContext, private val codegenDecorator: RustCodegenDecorator) :
    ServerCodegenVisitor(context, codegenDecorator) {

    init {
        val symbolVisitorConfig =
            SymbolVisitorConfig(
                runtimeConfig = settings.runtimeConfig,
                codegenConfig = settings.codegenConfig,
                handleRequired = true
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
        symbolProvider =
            codegenDecorator.symbolProvider(generator.symbolProvider(model, baseProvider))

        codegenContext = CodegenContext(model, symbolProvider, service, protocol, settings, mode = CodegenMode.Server)

        rustCrate = RustCrate(context.fileManifest, symbolProvider, DefaultPublicModules, settings.codegenConfig)
        protocolGenerator = protocolGeneratorFactory.buildProtocolGenerator(codegenContext)
    }

    override fun structureShape(shape: StructureShape) {
        logger.info("[python-server-codegen] Generating a structure $shape")
        rustCrate.useShapeWriter(shape) { writer ->
            PythonServerStructureGenerator(model, codegenContext, symbolProvider, writer, shape).render(CodegenTarget.SERVER)
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
        logger.info("[python-server-codegen] Generating an enum $shape")
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
     * This function _does not_ generate any serializers.
     */
    override fun unionShape(shape: UnionShape) {
        logger.info("[python-server-codegen] Generating an union $shape")
        rustCrate.useShapeWriter(shape) {
            UnionGenerator(model, symbolProvider, it, shape, renderUnknownVariant = false).render()
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
