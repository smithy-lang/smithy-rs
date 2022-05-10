/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.smithy.generators.client.FluentClientDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.util.deepMergeWith
import java.util.ServiceLoader
import java.util.logging.Logger

/**
 * [RustCodegenDecorator] allows downstream users to customize code generation.
 *
 * For example, AWS-specific code generation generates customizations required to support
 * AWS services. A different downstream customer may wish to add a different set of derive
 * attributes to the generated classes.
 */
interface RustCodegenDecorator {
    /**
     * The name of this [RustCodegenDecorator], used for logging and debug information
     */
    val name: String

    /**
     * Enable a deterministic ordering to be applied, with the lowest numbered integrations being applied first
     */
    val order: Byte

    fun configCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> = baseCustomizations

    fun operationCustomizations(
        codegenContext: CodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> = baseCustomizations

    fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> = baseCustomizations

    /**
     * Returns a map of Cargo.toml properties to change. For example, if a `homepage` needs to be
     * added to the Cargo.toml `[package]` section, a `mapOf("package" to mapOf("homepage", "https://example.com"))`
     * could be returned. Properties here overwrite the default properties.
     */
    fun crateManifestCustomizations(codegenContext: CodegenContext): ManifestCustomizations = emptyMap()

    fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {}

    fun protocols(serviceId: ShapeId, currentProtocols: ProtocolMap): ProtocolMap = currentProtocols

    fun transformModel(service: ServiceShape, model: Model): Model = model

    fun symbolProvider(baseProvider: RustSymbolProvider): RustSymbolProvider = baseProvider
}

/**
 * [CombinedCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
open class CombinedCodegenDecorator(decorators: List<RustCodegenDecorator>) : RustCodegenDecorator {
    private val orderedDecorators = decorators.sortedBy { it.order }
    override val name: String
        get() = "MetaDecorator"
    override val order: Byte
        get() = 0

    fun withDecorator(decorator: RustCodegenDecorator) = CombinedCodegenDecorator(orderedDecorators + decorator)

    override fun configCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: RustCodegenDecorator, customizations ->
            decorator.configCustomizations(codegenContext, customizations)
        }
    }

    override fun operationCustomizations(
        codegenContext: CodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: RustCodegenDecorator, customizations ->
            decorator.operationCustomizations(codegenContext, operation, customizations)
        }
    }

    override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator, customizations ->
            decorator.libRsCustomizations(
                codegenContext,
                customizations
            )
        }
    }

    override fun protocols(serviceId: ShapeId, currentProtocols: ProtocolMap): ProtocolMap {
        return orderedDecorators.foldRight(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(serviceId, protocolMap)
        }
    }

    override fun symbolProvider(baseProvider: RustSymbolProvider): RustSymbolProvider {
        return orderedDecorators.foldRight(baseProvider) { decorator, provider ->
            decorator.symbolProvider(provider)
        }
    }

    override fun crateManifestCustomizations(codegenContext: CodegenContext): ManifestCustomizations {
        return orderedDecorators.foldRight(emptyMap()) { decorator, customizations ->
            customizations.deepMergeWith(decorator.crateManifestCustomizations(codegenContext))
        }
    }

    override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        return orderedDecorators.forEach { it.extras(codegenContext, rustCrate) }
    }

    override fun transformModel(service: ServiceShape, model: Model): Model {
        return orderedDecorators.foldRight(model) { decorator, otherModel ->
            decorator.transformModel(service, otherModel)
        }
    }

    companion object {
        private val logger = Logger.getLogger("RustCodegenSPILoader")
        fun fromClasspath(context: PluginContext, vararg extras: RustCodegenDecorator): CombinedCodegenDecorator {
            val decorators = ServiceLoader.load(
                RustCodegenDecorator::class.java,
                context.pluginClassLoader.orElse(RustCodegenDecorator::class.java.classLoader)
            )
                .onEach {
                    logger.info("Adding Codegen Decorator: ${it.javaClass.name}")
                }.toList()
            return CombinedCodegenDecorator(decorators + RequiredCustomizations() + FluentClientDecorator() + extras)
        }
    }
}
