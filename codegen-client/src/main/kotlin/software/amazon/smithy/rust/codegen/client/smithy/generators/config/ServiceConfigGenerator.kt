/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators.config

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.docsOrFallback
import software.amazon.smithy.rust.codegen.core.rustlang.raw
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.util.hasTrait

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
 * class AddRegion : NamedCustomization<ServiceConfig>() {
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

    /**
     * The set default value of a field for use in tests, e.g `${configBuilderRef}.set_credentials(Credentials::for_tests())`
     */
    data class DefaultForTests(val configBuilderRef: String) : ServiceConfig("DefaultForTests")
}

data class ConfigParam(val name: String, val type: Symbol, val setterDocs: Writable?, val getterDocs: Writable? = null)

/**
 * Config customization for a config param with no special behavior:
 * 1. `pub(crate)` field
 * 2. convenience setter (non-optional)
 * 3. standard setter (&mut self)
 */
fun standardConfigParam(param: ConfigParam): ConfigCustomization = object : ConfigCustomization() {
    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.ConfigStruct -> writable {
                docsOrFallback(param.getterDocs)
                rust("pub (crate) ${param.name}: #T,", param.type.makeOptional())
            }

            ServiceConfig.ConfigImpl -> emptySection
            ServiceConfig.BuilderStruct -> writable {
                rust("${param.name}: #T,", param.type.makeOptional())
            }

            ServiceConfig.BuilderImpl -> writable {
                docsOrFallback(param.setterDocs)
                rust(
                    """
                    pub fn ${param.name}(mut self, ${param.name}: impl Into<#T>) -> Self {
                        self.${param.name} = Some(${param.name}.into());
                        self
                        }""",
                    param.type,
                )

                docsOrFallback(param.setterDocs)
                rust(
                    """
                    pub fn set_${param.name}(&mut self, ${param.name}: Option<#T>) -> &mut Self {
                        self.${param.name} = ${param.name};
                        self
                    }
                    """,
                    param.type,
                )
            }

            ServiceConfig.BuilderBuild -> writable {
                rust("${param.name}: self.${param.name},")
            }

            else -> emptySection
        }
    }
}

fun ServiceShape.needsIdempotencyToken(model: Model): Boolean {
    val operationIndex = OperationIndex.of(model)
    val topDownIndex = TopDownIndex.of(model)
    return topDownIndex.getContainedOperations(this.id).flatMap { operationIndex.getInputMembers(it).values }
        .any { it.hasTrait<IdempotencyTokenTrait>() }
}

typealias ConfigCustomization = NamedCustomization<ServiceConfig>

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
        fun withBaseBehavior(
            codegenContext: CodegenContext,
            extraCustomizations: List<ConfigCustomization>,
        ): ServiceConfigGenerator {
            val baseFeatures = mutableListOf<ConfigCustomization>()
            if (codegenContext.serviceShape.needsIdempotencyToken(codegenContext.model)) {
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
            writer.rustTemplate(
                """
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let mut config = f.debug_struct("Config");
                    config.finish()
                }
                """,
            )
        }

        writer.rustBlock("impl Config") {
            writer.rustTemplate(
                """
                /// Constructs a config builder.
                pub fn builder() -> Builder { Builder::default() }
                """,
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
            writer.docs("Constructs a config builder.")
            writer.rustTemplate("pub fn new() -> Self { Self::default() }")
            customizations.forEach {
                it.section(ServiceConfig.BuilderImpl)(this)
            }

            val testUtilOnly =
                Attribute(Attribute.cfg(Attribute.any(Attribute.feature(TestUtilFeature.name), writable("test"))))

            testUtilOnly.render(this)
            Attribute.AllowUnusedMut.render(this)
            docs("Apply test defaults to the builder")
            rustBlock("pub fn set_test_defaults(&mut self) -> &mut Self") {
                customizations.forEach { it.section(ServiceConfig.DefaultForTests("self"))(this) }
                rust("self")
            }

            testUtilOnly.render(this)
            Attribute.AllowUnusedMut.render(this)
            docs("Apply test defaults to the builder")
            rustBlock("pub fn with_test_defaults(mut self) -> Self") {
                rust("self.set_test_defaults(); self")
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
