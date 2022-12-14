/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.util.deepMergeWith
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.generators.protocol.ServerProtocolGenerator
import java.util.ServiceLoader
import java.util.logging.Logger

typealias ServerProtocolMap = ProtocolMap<ServerProtocolGenerator, ServerCodegenContext>

/**
 * [ServerCodegenDecorator] allows downstream users to customize code generation.
 *
 * For example, AWS-specific code generation generates customizations required to support
 * AWS services. A different downstream customer may wish to add a different set of derive
 * attributes to the generated classes.
 */
interface ServerCodegenDecorator {
    /**
     * The name of this [ServerCodegenDecorator], used for logging and debug information
     */
    val name: String

    /**
     * Enable a deterministic ordering to be applied, with the lowest numbered integrations being applied first
     */
    val order: Byte

    /**
     * Whether this decorator can be discovered on the classpath (defaults to true).
     * This is intended to only be overridden for decorators written specifically for tests.
     */
    fun classpathDiscoverable(): Boolean = true

    fun libRsCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations

    /**
     * Returns a map of Cargo.toml properties to change. For example, if a `homepage` needs to be
     * added to the Cargo.toml `[package]` section, a `mapOf("package" to mapOf("homepage", "https://example.com"))`
     * could be returned. Properties here overwrite the default properties.
     */
    fun crateManifestCustomizations(codegenContext: ServerCodegenContext): ManifestCustomizations = emptyMap()

    fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {}

    fun protocols(serviceId: ShapeId, currentProtocols: ServerProtocolMap): ServerProtocolMap = currentProtocols

    fun transformModel(service: ServiceShape, model: Model): Model = model
}

/**
 * [CombinedServerCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
class CombinedServerCodegenDecorator(decorators: List<ServerCodegenDecorator>) : ServerCodegenDecorator {
    private val orderedDecorators = decorators.sortedBy { it.order }
    override val name: String
        get() = "CombinedServerCodegenDecorator"
    override val order: Byte
        get() = 0

    override fun libRsCustomizations(
        codegenContext: ServerCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator, customizations ->
            decorator.libRsCustomizations(
                codegenContext,
                customizations,
            )
        }
    }

    override fun protocols(serviceId: ShapeId, currentProtocols: ServerProtocolMap): ServerProtocolMap {
        return orderedDecorators.foldRight(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(serviceId, protocolMap)
        }
    }

    override fun crateManifestCustomizations(codegenContext: ServerCodegenContext): ManifestCustomizations {
        return orderedDecorators.foldRight(emptyMap()) { decorator, customizations ->
            customizations.deepMergeWith(decorator.crateManifestCustomizations(codegenContext))
        }
    }

    override fun extras(codegenContext: ServerCodegenContext, rustCrate: RustCrate) {
        return orderedDecorators.forEach { it.extras(codegenContext, rustCrate) }
    }

    override fun transformModel(service: ServiceShape, model: Model): Model {
        return orderedDecorators.foldRight(model) { decorator, otherModel ->
            decorator.transformModel(service, otherModel)
        }
    }

    companion object {
        fun fromClasspath(
            context: PluginContext,
            vararg extras: ServerCodegenDecorator,
            logger: Logger = Logger.getLogger("RustServerCodegenSPILoader"),
        ): CombinedServerCodegenDecorator {
            val decorators = ServiceLoader.load(
                ServerCodegenDecorator::class.java,
                context.pluginClassLoader.orElse(ServerCodegenDecorator::class.java.classLoader),
            )

            val filteredDecorators = decorators.asSequence()
                .onEach { logger.info("Discovered Codegen Decorator: ${it.javaClass.name}") }
                .filter { it.classpathDiscoverable() }
                .onEach { logger.info("Adding Codegen Decorator: ${it.javaClass.name}") }
                .toList()
            return CombinedServerCodegenDecorator(filteredDecorators + extras)
        }
    }
}
