/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.util.deepMergeWith
import java.util.ServiceLoader
import java.util.logging.Logger

typealias ClientProtocolMap = ProtocolMap<ClientProtocolGenerator, ClientCodegenContext>

/**
 * [ClientCodegenDecorator] allows downstream users to customize code generation.
 *
 * For example, AWS-specific code generation generates customizations required to support
 * AWS services. A different downstream customer may wish to add a different set of derive
 * attributes to the generated classes.
 */
interface ClientCodegenDecorator {
    /**
     * The name of this [ClientCodegenDecorator], used for logging and debug information
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

    fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations

    fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations

    fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations

    /**
     * Returns a map of Cargo.toml properties to change. For example, if a `homepage` needs to be
     * added to the Cargo.toml `[package]` section, a `mapOf("package" to mapOf("homepage", "https://example.com"))`
     * could be returned. Properties here overwrite the default properties.
     */
    fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations = emptyMap()

    fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {}

    fun protocols(serviceId: ShapeId, currentProtocols: ClientProtocolMap): ClientProtocolMap = currentProtocols

    fun transformModel(service: ServiceShape, model: Model): Model = model

    fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> = listOf()
}

/**
 * [CombinedClientCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
open class CombinedClientCodegenDecorator(decorators: List<ClientCodegenDecorator>) : ClientCodegenDecorator {
    private val orderedDecorators = decorators.sortedBy { it.order }
    override val name: String
        get() = "CombinedClientCodegenDecorator"
    override val order: Byte
        get() = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: ClientCodegenDecorator, customizations ->
            decorator.configCustomizations(codegenContext, customizations)
        }
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: ClientCodegenDecorator, customizations ->
            decorator.operationCustomizations(codegenContext, operation, customizations)
        }
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator, customizations ->
            decorator.libRsCustomizations(
                codegenContext,
                customizations,
            )
        }
    }

    override fun protocols(serviceId: ShapeId, currentProtocols: ClientProtocolMap): ClientProtocolMap {
        return orderedDecorators.foldRight(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(serviceId, protocolMap)
        }
    }

    override fun crateManifestCustomizations(codegenContext: ClientCodegenContext): ManifestCustomizations {
        return orderedDecorators.foldRight(emptyMap()) { decorator, customizations ->
            customizations.deepMergeWith(decorator.crateManifestCustomizations(codegenContext))
        }
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        return orderedDecorators.forEach { it.extras(codegenContext, rustCrate) }
    }

    override fun transformModel(service: ServiceShape, model: Model): Model {
        return orderedDecorators.foldRight(model) { decorator, otherModel ->
            decorator.transformModel(service, otherModel)
        }
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return orderedDecorators.flatMap { it.endpointCustomizations(codegenContext) }
    }

    companion object {
        fun fromClasspath(
            context: PluginContext,
            vararg extras: ClientCodegenDecorator,
            logger: Logger = Logger.getLogger("RustClientCodegenSPILoader"),
        ): CombinedClientCodegenDecorator {
            val decorators = ServiceLoader.load(
                ClientCodegenDecorator::class.java,
                context.pluginClassLoader.orElse(ClientCodegenDecorator::class.java.classLoader),
            )

            val filteredDecorators = decorators.asSequence()
                .onEach { logger.info("Discovered Codegen Decorator: ${it.javaClass.name}") }
                .filter { it.classpathDiscoverable() }
                .onEach { logger.info("Adding Codegen Decorator: ${it.javaClass.name}") }
                .toList()
            return CombinedClientCodegenDecorator(filteredDecorators + extras)
        }
    }
}
