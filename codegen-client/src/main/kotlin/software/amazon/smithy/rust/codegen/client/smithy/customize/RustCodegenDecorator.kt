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
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.protocols.ProtocolMap
import software.amazon.smithy.rust.codegen.core.util.deepMergeWith
import java.util.ServiceLoader
import java.util.logging.Logger

/**
 * [RustCodegenDecorator] allows downstream users to customize code generation.
 *
 * For example, AWS-specific code generation generates customizations required to support
 * AWS services. A different downstream customer may wish to add a different set of derive
 * attributes to the generated classes.
 */
interface RustCodegenDecorator<T, C : CodegenContext> {
    /**
     * The name of this [RustCodegenDecorator], used for logging and debug information
     */
    val name: String

    /**
     * Enable a deterministic ordering to be applied, with the lowest numbered integrations being applied first
     */
    val order: Byte

    fun configCustomizations(
        codegenContext: C,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations

    // This is only used by decorators for smithy-rs _clients_.
    fun operationCustomizations(
        codegenContext: C,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations

    fun libRsCustomizations(
        codegenContext: C,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations

    /**
     * Returns a map of Cargo.toml properties to change. For example, if a `homepage` needs to be
     * added to the Cargo.toml `[package]` section, a `mapOf("package" to mapOf("homepage", "https://example.com"))`
     * could be returned. Properties here overwrite the default properties.
     */
    fun crateManifestCustomizations(codegenContext: C): ManifestCustomizations = emptyMap()

    fun extras(codegenContext: C, rustCrate: RustCrate) {}

    fun protocols(serviceId: ShapeId, currentProtocols: ProtocolMap<T, C>): ProtocolMap<T, C> = currentProtocols

    fun transformModel(service: ServiceShape, model: Model): Model = model

    fun endpointCustomizations(codegenContext: C): List<EndpointCustomization> = listOf()

    fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean
}

/**
 * [CombinedCodegenDecorator] merges the results of multiple decorators into a single decorator.
 *
 * This makes the actual concrete codegen simpler by not needing to deal with multiple separate decorators.
 */
open class CombinedCodegenDecorator<T, C : CodegenContext>(decorators: List<RustCodegenDecorator<T, C>>) :
    RustCodegenDecorator<T, C> {
    private val orderedDecorators = decorators.sortedBy { it.order }
    override val name: String
        get() = "MetaDecorator"
    override val order: Byte
        get() = 0

    fun withDecorator(vararg decorator: RustCodegenDecorator<T, C>) =
        CombinedCodegenDecorator(orderedDecorators + decorator)

    override fun configCustomizations(
        codegenContext: C,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: RustCodegenDecorator<T, C>, customizations ->
            decorator.configCustomizations(codegenContext, customizations)
        }
    }

    override fun operationCustomizations(
        codegenContext: C,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator: RustCodegenDecorator<T, C>, customizations ->
            decorator.operationCustomizations(codegenContext, operation, customizations)
        }
    }

    override fun libRsCustomizations(
        codegenContext: C,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return orderedDecorators.foldRight(baseCustomizations) { decorator, customizations ->
            decorator.libRsCustomizations(
                codegenContext,
                customizations,
            )
        }
    }

    override fun protocols(serviceId: ShapeId, currentProtocols: ProtocolMap<T, C>): ProtocolMap<T, C> {
        return orderedDecorators.foldRight(currentProtocols) { decorator, protocolMap ->
            decorator.protocols(serviceId, protocolMap)
        }
    }

    override fun crateManifestCustomizations(codegenContext: C): ManifestCustomizations {
        return orderedDecorators.foldRight(emptyMap()) { decorator, customizations ->
            customizations.deepMergeWith(decorator.crateManifestCustomizations(codegenContext))
        }
    }

    override fun extras(codegenContext: C, rustCrate: RustCrate) {
        return orderedDecorators.forEach { it.extras(codegenContext, rustCrate) }
    }

    override fun transformModel(service: ServiceShape, model: Model): Model {
        return orderedDecorators.foldRight(model) { decorator, otherModel ->
            decorator.transformModel(service, otherModel)
        }
    }

    override fun endpointCustomizations(codegenContext: C): List<EndpointCustomization> {
        return orderedDecorators.flatMap { it.endpointCustomizations(codegenContext) }
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        // `CombinedCodegenDecorator` can work with all types of codegen context.
        CodegenContext::class.java.isAssignableFrom(clazz)

    companion object {
        inline fun <T, reified C : CodegenContext> fromClasspath(
            context: PluginContext,
            vararg extras: RustCodegenDecorator<T, C>,
            logger: Logger = Logger.getLogger("RustCodegenSPILoader"),
        ): CombinedCodegenDecorator<T, C> {
            val decorators = ServiceLoader.load(
                RustCodegenDecorator::class.java,
                context.pluginClassLoader.orElse(RustCodegenDecorator::class.java.classLoader),
            )

            val filteredDecorators = filterDecorators<T, C>(decorators, logger).toList()
            return CombinedCodegenDecorator(filteredDecorators + extras)
        }

        /*
         * This function has been extracted solely for the purposes of easily unit testing the important filtering logic.
         * Unfortunately, it must be part of the public API because public API inline functions are not allowed to use
         * non-public-API declarations.
         * See https://kotlinlang.org/docs/inline-functions.html#restrictions-for-public-api-inline-functions.
         */
        inline fun <T, reified C : CodegenContext> filterDecorators(
            decorators: Iterable<RustCodegenDecorator<*, *>>,
            logger: Logger = Logger.getLogger("RustCodegenSPILoader"),
        ): Sequence<RustCodegenDecorator<T, C>> =
            decorators.asSequence()
                .onEach {
                    logger.info("Discovered Codegen Decorator: ${it.javaClass.name}")
                }
                // The JVM's `ServiceLoader` is woefully underpowered in that it can not load classes with generic
                // parameters with _fixed_ parameters (like what we're trying to do here; we only want `RustCodegenDecorator`
                // classes with code-generation context matching the input `T`).
                // There are various workarounds: https://stackoverflow.com/questions/5451734/loading-generic-service-implementations-via-java-util-serviceloader
                // All involve loading _all_ classes from the classpath (i.e. all `RustCodegenDecorator<*>`), and then
                // filtering them.
                // Note that attempting to downcast a generic class `C<T>` to `C<U>` where `U: T` is not possible to do
                // in Kotlin (and presumably all JVM-based languages) _at runtime_. Not even when using reified type
                // parameters of inline functions. See https://kotlinlang.org/docs/generics.html#type-erasure for details.
                .filter {
                    val clazz = C::class.java
                    it.supportsCodegenContext(clazz)
                }
                .onEach {
                    logger.info("Adding Codegen Decorator: ${it.javaClass.name}")
                }
                .map {
                    // Cast is safe because of the filter above.
                    // Not that it really has an effect at runtime, since its unchecked.
                    @Suppress("UNCHECKED_CAST")
                    it as RustCodegenDecorator<T, C>
                }
    }
}
