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
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations
import software.amazon.smithy.rust.codegen.core.smithy.makeOptional
import software.amazon.smithy.rust.codegen.core.util.hasTrait
import software.amazon.smithy.rust.codegen.core.util.letIf

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
     * A section for customizing individual fields in the initializer of Config
     */
    object BuilderBuildExtras : ServiceConfig("BuilderBuildExtras")

    /**
     * A section for setting up a field to be used by ConfigOverrideRuntimePlugin
     */
    data class OperationConfigOverride(val cfg: String) : ServiceConfig("ToRuntimePlugin")

    /**
     * A section for appending additional runtime plugins, stored in [interceptorsField], to [interceptors]
     */
    data class RuntimePluginInterceptors(val interceptors: String, val interceptorsField: String) : ServiceConfig("ToRuntimePluginInterceptors")

    /**
     * A section for extra functionality that needs to be defined with the config module
     */
    object Extras : ServiceConfig("Extras")

    /**
     * The set default value of a field for use in tests, e.g `${configBuilderRef}.set_credentials(Credentials::for_tests())`
     */
    data class DefaultForTests(val configBuilderRef: String) : ServiceConfig("DefaultForTests")
}

data class ConfigParam(
    val name: String,
    val type: Symbol,
    val newtype: RuntimeType?,
    val setterDocs: Writable?,
    val getterDocs: Writable? = null,
    val optional: Boolean = true,
) {

    data class Builder(
        var name: String? = null,
        var type: Symbol? = null,
        var newtype: RuntimeType? = null,
        var setterDocs: Writable? = null,
        var getterDocs: Writable? = null,
        var optional: Boolean = true,
    ) {
        fun name(name: String) = apply { this.name = name }
        fun type(type: Symbol) = apply { this.type = type }
        fun newtype(newtype: RuntimeType) = apply { this.newtype = newtype }
        fun setterDocs(setterDocs: Writable?) = apply { this.setterDocs = setterDocs }
        fun getterDocs(getterDocs: Writable?) = apply { this.getterDocs = getterDocs }
        fun optional(optional: Boolean) = apply { this.optional = optional }
        fun build() = ConfigParam(name!!, type!!, newtype, setterDocs, getterDocs, optional)
    }
}

/**
 * Generate a [RuntimeType] for a newtype whose name is [newtypeName] that wraps [inner].
 *
 * When config parameters are stored in a config map in Rust, stored parameters are keyed by type.
 * Therefore, primitive types, such as bool and String, need to be wrapped in newtypes to make them distinct.
 */
fun configParamNewtype(newtypeName: String, inner: Symbol, runtimeConfig: RuntimeConfig) =
    RuntimeType.forInlineFun(newtypeName, ClientRustModule.config) {
        val codegenScope = arrayOf(
            "Storable" to RuntimeType.smithyTypes(runtimeConfig).resolve("config_bag::Storable"),
            "StoreReplace" to RuntimeType.smithyTypes(runtimeConfig).resolve("config_bag::StoreReplace"),
        )
        rustTemplate(
            """
            ##[derive(Debug, Clone)]
            pub(crate) struct $newtypeName(pub(crate) $inner);
            impl #{Storable} for $newtypeName {
                type Storer = #{StoreReplace}<Self>;
            }
            """,
            *codegenScope,
        )
    }

/**
 * Render an expression that loads a value from a config bag.
 *
 * The expression to be rendered handles a case where a newtype is stored in the config bag, but the user expects
 * the underlying raw type after the newtype has been loaded from the bag.
 */
fun loadFromConfigBag(innerTypeName: String, newtype: RuntimeType): Writable = writable {
    rustTemplate(
        """
        load::<#{newtype}>().map(#{f})
        """,
        "newtype" to newtype,
        "f" to writable {
            if (innerTypeName == "bool") {
                rust("|ty| ty.0")
            } else {
                rust("|ty| ty.0.clone()")
            }
        },
    )
}

/**
 * Config customization for a config param with no special behavior:
 * 1. `pub(crate)` field
 * 2. convenience setter (non-optional)
 * 3. standard setter (&mut self)
 */
fun standardConfigParam(param: ConfigParam, codegenContext: ClientCodegenContext): ConfigCustomization = object : ConfigCustomization() {
    private val runtimeMode = codegenContext.smithyRuntimeMode

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            ServiceConfig.ConfigStruct -> writable {
                if (runtimeMode.defaultToMiddleware) {
                    docsOrFallback(param.getterDocs)
                    val t = when (param.optional) {
                        true -> param.type.makeOptional()
                        false -> param.type
                    }
                    rust("pub (crate) ${param.name}: #T,", t)
                }
            }

            ServiceConfig.BuilderStruct -> writable {
                if (runtimeMode.defaultToMiddleware) {
                    rust("${param.name}: #T,", param.type.makeOptional())
                }
            }

            ServiceConfig.BuilderImpl -> writable {
                docsOrFallback(param.setterDocs)
                rust(
                    """
                    pub fn ${param.name}(mut self, ${param.name}: impl Into<#T>) -> Self {
                        self.set_${param.name}(Some(${param.name}.into()));
                        self
                    }""",
                    param.type,
                )

                docsOrFallback(param.setterDocs)
                if (runtimeMode.defaultToOrchestrator) {
                    rustTemplate(
                        """
                        pub fn set_${param.name}(&mut self, ${param.name}: Option<#{T}>) -> &mut Self {
                            self.inner.store_or_unset(${param.name}.map(#{newtype}));
                            self
                        }
                        """,
                        "T" to param.type,
                        "newtype" to param.newtype!!,
                    )
                } else {
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
            }

            ServiceConfig.BuilderBuild -> writable {
                if (runtimeMode.defaultToMiddleware) {
                    val default = "".letIf(!param.optional) { ".unwrap_or_default() " }
                    rust("${param.name}: self.${param.name}$default,")
                }
            }

            is ServiceConfig.OperationConfigOverride -> emptySection

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

class ServiceConfigGenerator(
    codegenContext: ClientCodegenContext,
    private val customizations: List<ConfigCustomization> = listOf(),
) {
    companion object {
        fun withBaseBehavior(
            codegenContext: ClientCodegenContext,
            extraCustomizations: List<ConfigCustomization>,
        ): ServiceConfigGenerator {
            val baseFeatures = mutableListOf<ConfigCustomization>()
            if (codegenContext.serviceShape.needsIdempotencyToken(codegenContext.model)) {
                baseFeatures.add(IdempotencyTokenProviderCustomization(codegenContext))
            }
            return ServiceConfigGenerator(codegenContext, baseFeatures + extraCustomizations)
        }
    }

    private val runtimeApi = RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig)
    private val smithyTypes = RuntimeType.smithyTypes(codegenContext.runtimeConfig)
    val codegenScope = arrayOf(
        "BoxError" to RuntimeType.boxError(codegenContext.runtimeConfig),
        "CloneableLayer" to smithyTypes.resolve("config_bag::CloneableLayer"),
        "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
        "ConfigBagAccessors" to RuntimeType.configBagAccessors(codegenContext.runtimeConfig),
        "FrozenLayer" to smithyTypes.resolve("config_bag::FrozenLayer"),
        "InterceptorRegistrar" to runtimeApi.resolve("client::interceptors::InterceptorRegistrar"),
        "Layer" to smithyTypes.resolve("config_bag::Layer"),
        "RuntimePlugin" to runtimeApi.resolve("client::runtime_plugin::RuntimePlugin"),
        *preludeScope,
    )
    private val moduleUseName = codegenContext.moduleUseName()
    private val runtimeMode = codegenContext.smithyRuntimeMode

    fun render(writer: RustWriter) {
        writer.docs("Service config.\n")
        customizations.forEach {
            it.section(ServiceConfig.ConfigStructAdditionalDocs)(writer)
        }
        Attribute(Attribute.derive(RuntimeType.Clone)).render(writer)
        writer.rustBlock("pub struct Config") {
            if (runtimeMode.defaultToOrchestrator) {
                rustTemplate(
                    "inner: #{FrozenLayer},",
                    *codegenScope,
                )
            }
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
        writer.raw("#[derive(Clone, Default)]")
        writer.rustBlock("pub struct Builder") {
            if (runtimeMode.defaultToOrchestrator) {
                rustTemplate(
                    "inner: #{CloneableLayer},",
                    *codegenScope,
                )
            }
            customizations.forEach {
                it.section(ServiceConfig.BuilderStruct)(this)
            }
        }

        // Custom implementation for Debug so we don't need to enforce Debug down the chain
        writer.rustBlock("impl std::fmt::Debug for Builder") {
            writer.rustTemplate(
                """
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let mut config = f.debug_struct("Builder");
                    config.finish()
                }
                """,
            )
        }

        writer.rustBlock("impl Builder") {
            writer.docs("Constructs a config builder.")
            writer.rust("pub fn new() -> Self { Self::default() }")
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
            if (runtimeMode.defaultToOrchestrator) {
                rust("##[allow(unused_mut)]")
                rustBlock("pub fn build(mut self) -> Config") {
                    rustTemplate(
                        """
                        ##[allow(unused_imports)]
                        use #{ConfigBagAccessors};
                        // The builder is being turned into a service config. While doing so, we'd like to avoid
                        // requiring that items created and stored _during_ the build method be `Clone`, since they
                        // will soon be part of a `FrozenLayer` owned by the service config. So we will convert the
                        // current `CloneableLayer` into a `Layer` that does not impose the `Clone` requirement.
                        let layer: #{Layer} = self
                            .inner
                            .into();
                        let mut layer = layer.with_name("$moduleUseName::config::config");
                        """,
                        *codegenScope,
                    )
                    customizations.forEach {
                        it.section(ServiceConfig.BuilderBuild)(this)
                    }
                    rustBlock("Config") {
                        customizations.forEach {
                            it.section(ServiceConfig.BuilderBuildExtras)(this)
                        }
                        rust("inner: layer.freeze(),")
                    }
                }
            } else {
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

    fun renderRuntimePluginImplForSelf(writer: RustWriter) {
        writer.rustTemplate(
            """
            impl #{RuntimePlugin} for Config {
                fn config(&self) -> #{Option}<#{FrozenLayer}> {
                    #{Some}(self.inner.clone())
                }

                fn interceptors(&self, _interceptors: &mut #{InterceptorRegistrar}) {
                    #{interceptors}
                }
            }

            """,
            *codegenScope,
            "config" to writable { writeCustomizations(customizations, ServiceConfig.OperationConfigOverride("cfg")) },
            "interceptors" to writable {
                writeCustomizations(customizations, ServiceConfig.RuntimePluginInterceptors("_interceptors", "self"))
            },
        )
    }
}
