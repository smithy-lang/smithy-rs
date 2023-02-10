/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.core.smithy.customize

import software.amazon.smithy.build.PluginContext
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.ManifestCustomizations
import software.amazon.smithy.rust.codegen.core.util.deepMergeWith
import java.util.ServiceLoader
import java.util.logging.Logger

/**
 * Represents the bare minimum for codegen plugin customization.
 */
interface CoreCodegenDecorator<CodegenContext> {
    /**
     * The name of this decorator, used for logging and debug information
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

    /**
     * Hook to transform the Smithy model before codegen takes place.
     */
    fun transformModel(service: ServiceShape, model: Model): Model = model

    /**
     * Hook to add additional modules to the generated crate.
     */
    fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {}

    /**
     * Hook to customize the generated `Cargo.toml` file.
     *
     * Returns a map of Cargo.toml properties to change. For example, if a `homepage` needs to be
     * added to the Cargo.toml `[package]` section, a `mapOf("package" to mapOf("homepage", "https://example.com"))`
     * could be returned. Properties here overwrite the default properties.
     */
    fun crateManifestCustomizations(codegenContext: CodegenContext): ManifestCustomizations = emptyMap()

    /**
     * Hook to customize the generated `lib.rs` file.
     */
    fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> = baseCustomizations

    /**
     * Extra sections allow one decorator to influence another. This is intended to be used by querying the `rootDecorator`
     */
    fun extraSections(codegenContext: CodegenContext): List<AdHocCustomization> = listOf()
}

/**
 * Implementations for combining decorators for the core customizations.
 */
abstract class CombinedCoreCodegenDecorator<CodegenContext, Decorator : CoreCodegenDecorator<CodegenContext>>(
    decorators: List<Decorator>,
) : CoreCodegenDecorator<CodegenContext> {
    private val orderedDecorators = decorators.sortedBy { it.order }

    final override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> =
        combineCustomizations(baseCustomizations) { decorator, customizations ->
            decorator.libRsCustomizations(codegenContext, customizations)
        }

    final override fun crateManifestCustomizations(codegenContext: CodegenContext): ManifestCustomizations =
        combineCustomizations(emptyMap()) { decorator, customizations ->
            customizations.deepMergeWith(decorator.crateManifestCustomizations(codegenContext))
        }

    final override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        return orderedDecorators.forEach { it.extras(codegenContext, rustCrate) }
    }

    final override fun transformModel(service: ServiceShape, model: Model): Model =
        combineCustomizations(model) { decorator, otherModel ->
            decorator.transformModel(otherModel.expectShape(service.id, ServiceShape::class.java), otherModel)
        }

    final override fun extraSections(codegenContext: CodegenContext): List<AdHocCustomization> =
        addCustomizations { decorator -> decorator.extraSections(codegenContext) }

    /**
     * Combines customizations from multiple ordered codegen decorators.
     *
     * Using this combinator allows for customizations to remove other customizations since the `mergeCustomizations`
     * function can mutate the entire returned customization list.
     */
    protected fun <CombinedCustomizations> combineCustomizations(
        /** Initial customizations. These will remain at the front of the list unless `mergeCustomizations` changes order. */
        baseCustomizations: CombinedCustomizations,
        /** A function that retrieves customizations from a decorator and combines them with the given customizations. */
        mergeCustomizations: (Decorator, CombinedCustomizations) -> CombinedCustomizations,
    ): CombinedCustomizations =
        orderedDecorators.foldRight(baseCustomizations) { decorator, customizations ->
            mergeCustomizations(decorator, customizations)
        }

    /**
     * Combines customizations from multiple ordered codegen decorators in a purely additive way.
     *
     * Unlike `combineCustomizations`, customizations combined in this way cannot remove other customizations.
     */
    protected fun <Customization> addCustomizations(
        /** Returns customizations from a decorator. */
        getCustomizations: (Decorator) -> List<Customization>,
    ): List<Customization> = orderedDecorators.flatMap(getCustomizations)

    companion object {
        /**
         * Loads decorators of the given class from the classpath and filters them by the `classpathDiscoverable`
         * method on `CoreCodegenDecorator`.
         */
        @JvmStatic
        protected fun <CodegenContext, Decorator : CoreCodegenDecorator<CodegenContext>> decoratorsFromClasspath(
            context: PluginContext,
            decoratorClass: Class<Decorator>,
            logger: Logger,
            vararg extras: Decorator,
        ): List<Decorator> {
            val decorators = ServiceLoader.load(
                decoratorClass,
                context.pluginClassLoader.orElse(decoratorClass.classLoader),
            )

            val filteredDecorators = decorators.asSequence()
                .onEach { logger.info("Discovered Codegen Decorator: ${it!!::class.java.name}") }
                .filter { it!!.classpathDiscoverable() }
                .onEach { logger.info("Adding Codegen Decorator: ${it!!::class.java.name}") }
                .toList()
            return filteredDecorators + extras
        }
    }
}
