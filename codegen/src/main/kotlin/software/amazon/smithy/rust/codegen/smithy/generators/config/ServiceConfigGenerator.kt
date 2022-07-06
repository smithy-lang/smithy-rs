/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.generators.config

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.docs
import software.amazon.smithy.rust.codegen.rustlang.raw
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.smithy.customize.Section
import software.amazon.smithy.rust.codegen.util.hasTrait

/**
 * [ServiceConfig] is the parent type of sections that can be overridden when generating a config for a service.
 *
 * This functions similar to a strongly typed version of `CodeWriter`'s named sections
 * (and may eventually be modified to use that for implementation), however, it's currently strictly additive.
 *
 * As this becomes used for more complex customizations, the ability to fully replace the contents will become
 * necessary.
 *
 * Usage:
 * ```kotlin
 * class AddRegion : NamedSectionGenerator<ServiceConfig>() {
 *  override fun section(section: ServiceConfig): Writable {
 *    return when (section) {
 *      is ServiceConfig.ConfigStruct -> writeable {
 *          rust("pub (crate) region: String,")
 *      }
 *      else -> emptySection
 *    }
 * }
 * ```
 */
sealed class ServiceConfig(name: String) : Section(name) {
    /**
     * Additional documentation comments for the `Config` struct.
     */
    object ConfigStructAdditionalDocs : ServiceConfig("ConfigStructAdditionalDocs")

    /**
     * Struct definition of `Config`. Fields should end with `,` (e.g. `foo: Box<u64>,`)
     */
    object ConfigStruct : ServiceConfig("ConfigStruct")

    /**
     * impl block of `Config`. (e.g. to add functions)
     * e.g.
     * ```kotlin
     * rust("pub fn is_cross_region() -> bool { true }")
     * ```
     */
    object ConfigImpl : ServiceConfig("ConfigImpl")

    /** Struct definition of `ConfigBuilder` **/
    object BuilderStruct : ServiceConfig("BuilderStruct")

    /** impl block of `ConfigBuilder` **/
    object BuilderImpl : ServiceConfig("BuilderImpl")

    /**
     * Convert from a field in the builder to the final field in config
     *  e.g.
     *  ```kotlin
     *  rust("""my_field: my_field.unwrap_or_else(||"default")""")
     *  ```
     */
    object BuilderBuild : ServiceConfig("BuilderBuild")

    /**
     * A section for extra functionality that needs to be defined with the config module
     */
    object Extras : ServiceConfig("Extras")
}

fun ServiceShape.needsIdempotencyToken(model: Model): Boolean {
    val operationIndex = OperationIndex.of(model)
    val topDownIndex = TopDownIndex.of(model)
    return topDownIndex.getContainedOperations(this.id).flatMap { operationIndex.getInputMembers(it).values }.any { it.hasTrait<IdempotencyTokenTrait>() }
}

typealias ConfigCustomization = NamedSectionGenerator<ServiceConfig>

/**
 * Generate a `Config` struct, implementation & builder for a given service, approximately:
 * ```rust
 * struct Config {
 *    // various members
 * }
 * impl Config {
 *    // some public functions
 * }
 *
 * struct ConfigBuilder {
 *    // generally, optional members corresponding to members of `Config`
 * }
 * impl ConfigBuilder {
 *    // builder implementation
 * }
 */
class ServiceConfigGenerator(private val customizations: List<ConfigCustomization> = listOf()) {

    companion object {
        fun withBaseBehavior(coreCodegenContext: CoreCodegenContext, extraCustomizations: List<ConfigCustomization>): ServiceConfigGenerator {
            val baseFeatures = mutableListOf<ConfigCustomization>()
            if (coreCodegenContext.serviceShape.needsIdempotencyToken(coreCodegenContext.model)) {
                baseFeatures.add(IdempotencyTokenProviderCustomization())
            }
            return ServiceConfigGenerator(baseFeatures + extraCustomizations)
        }
    }

    fun render(writer: RustWriter) {
        writer.docs("Service config.\n")
        customizations.forEach {
            it.section(ServiceConfig.ConfigStructAdditionalDocs)(writer)
        }
        writer.rustBlock("pub struct Config") {
            customizations.forEach {
                it.section(ServiceConfig.ConfigStruct)(this)
            }
        }

        // Custom implementation for Debug so we don't need to enforce Debug down the chain
        writer.rustBlock("impl std::fmt::Debug for Config") {
            rustTemplate(
                """
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let mut config = f.debug_struct("Config");
                    config.finish()
                }
                """
            )
        }

        writer.rustBlock("impl Config") {
            rustTemplate(
                """
                /// Constructs a config builder.
                pub fn builder() -> Builder { Builder::default() }
                """
            )
            customizations.forEach {
                it.section(ServiceConfig.ConfigImpl)(this)
            }
        }

        writer.docs("Builder for creating a `Config`.")
        writer.raw("#[derive(Default)]")
        writer.rustBlock("pub struct Builder") {
            customizations.forEach {
                it.section(ServiceConfig.BuilderStruct)(this)
            }
        }
        writer.rustBlock("impl Builder") {
            docs("Constructs a config builder.")
            rustTemplate("pub fn new() -> Self { Self::default() }")
            customizations.forEach {
                it.section(ServiceConfig.BuilderImpl)(this)
            }
            docs("Builds a [`Config`].")
            rustBlock("pub fn build(self) -> Config") {
                rustBlock("Config") {
                    customizations.forEach {
                        it.section(ServiceConfig.BuilderBuild)(this)
                    }
                }
            }
        }
        customizations.forEach {
            it.section(ServiceConfig.Extras)(writer)
        }
    }
}
