/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy.generators.config

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.raw
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.customize.NamedSectionGenerator
import software.amazon.smithy.rust.codegen.smithy.customize.Section
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

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
    /** Struct definition of `Config`. Fields should end with `,`
     *  eg. `foo: Box<u64>,`
     **/
    object ConfigStruct : ServiceConfig("ConfigStruct")
    /** impl block of `Config`. (eg. to add functions)
     * eg.
     * ```kotlin
     * rust("pub fn is_cross_region() -> bool { true }")
     * ```
     */
    object ConfigImpl : ServiceConfig("ConfigImpl")

    /** Struct definition of `ConfigBuilder` **/
    object BuilderStruct : ServiceConfig("BuilderStruct")
    /** impl block of `ConfigBuilder` **/
    object BuilderImpl : ServiceConfig("BuilderImpl")
    /** Convert from a field in the builder to the final field in config
     *  eg.
     *  ```kotlin
     *  rust("""my_field: my_field.unwrap_or_else(||"default")""")
     *  ```
     **/
    object BuilderBuild : ServiceConfig("BuilderBuild")
}

// TODO: if this becomes hot, it may need to be cached in a knowledge index
fun ServiceShape.needsIdempotencyToken(model: Model): Boolean {
    val operationIndex = OperationIndex.of(model)
    return this.allOperations.flatMap { operationIndex.getInputMembers(it).values }.any { it.hasTrait(IdempotencyTokenTrait::class.java) }
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
        fun withBaseBehavior(protocolConfig: ProtocolConfig, extraCustomizations: List<ConfigCustomization>): ServiceConfigGenerator {
            val baseFeatures = mutableListOf<ConfigCustomization>()
            if (protocolConfig.serviceShape.needsIdempotencyToken(protocolConfig.model)) {
                baseFeatures.add(IdempotencyTokenProviderCustomization())
            }
            return ServiceConfigGenerator(baseFeatures + extraCustomizations)
        }
    }

    fun render(writer: RustWriter) {
        writer.rustBlock("pub struct Config") {
            customizations.forEach {
                it.section(ServiceConfig.ConfigStruct)(this)
            }
        }

        writer.rustBlock("impl Config") {
            rustTemplate(
                """
                pub fn builder() -> Builder { Builder::default() }
            """
            )
            customizations.forEach {
                it.section(ServiceConfig.ConfigImpl)(this)
            }
        }

        writer.raw("#[derive(Default)]")
        writer.rustBlock("pub struct Builder") {
            customizations.forEach {
                it.section(ServiceConfig.BuilderStruct)(this)
            }
        }
        writer.rustBlock("impl Builder") {
            rustTemplate("pub fn new() -> Self { Self::default() }")
            customizations.forEach {
                it.section(ServiceConfig.BuilderImpl)(this)
            }
            rustBlock("pub fn build(self) -> Config") {
                rustBlock("Config") {
                    customizations.forEach {
                        it.section(ServiceConfig.BuilderBuild)(this)
                    }
                }
            }
        }
    }
}
