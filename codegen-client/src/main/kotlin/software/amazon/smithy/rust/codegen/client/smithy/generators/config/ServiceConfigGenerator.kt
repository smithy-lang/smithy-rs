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
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.AttributeKind
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.docsOrFallback
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section
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
    data object ConfigStructAdditionalDocs : ServiceConfig("ConfigStructAdditionalDocs")

    /**
     * Struct definition of `Config`. Fields should end with `,` (e.g. `foo: Box<u64>,`)
     */
    data object ConfigStruct : ServiceConfig("ConfigStruct")

    /**
     * impl block of `Config`. (e.g. to add functions)
     * e.g.
     * ```kotlin
     * rust("pub fn is_cross_region() -> bool { true }")
     * ```
     */
    data object ConfigImpl : ServiceConfig("ConfigImpl")

    /** Struct definition of `ConfigBuilder` **/
    data object BuilderStruct : ServiceConfig("BuilderStruct")

    /** impl block of `ConfigBuilder` **/
    data object BuilderImpl : ServiceConfig("BuilderImpl")

    // It is important to ensure through type system that each field added to config implements this injection,
    // tracked by smithy-rs#3419

    /**
     * Load a value from a config bag and store it in ConfigBuilder
     *  e.g.
     *  ```kotlin
     *  rust("""builder.set_field(config_bag.load::<FieldType>().cloned())""")
     *  ```
     */
    data class BuilderFromConfigBag(val builder: String, val configBag: String) : ServiceConfig("BuilderFromConfigBag")

    /**
     * Convert from a field in the builder to the final field in config
     *  e.g.
     *  ```kotlin
     *  rust("""my_field: my_field.unwrap_or_else(||"default")""")
     *  ```
     */
    data object BuilderBuild : ServiceConfig("BuilderBuild")

    /**
     * A section for setting up a field to be used by ConfigOverrideRuntimePlugin
     */
    data class OperationConfigOverride(val cfg: String) : ServiceConfig("ToRuntimePlugin")

    /**
     * A section for extra functionality that needs to be defined with the config module
     */
    data object Extras : ServiceConfig("Extras")

    /**
     * The set default value of a field for use in tests, e.g `${configBuilderRef}.set_credentials(Credentials::for_tests())`
     */
    data class DefaultForTests(val configBuilderRef: String) : ServiceConfig("DefaultForTests")

    /**
     * Set default value of a field for use in tests, e.g `${configBuilderRef}.set_credentials(Credentials::for_tests())`
     *
     * NOTE: V2 was added for backwards compatibility
     */
    data class DefaultForTestsV2(val configBuilderRef: String) : ServiceConfig("DefaultForTestsV2")
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
fun configParamNewtype(
    newtypeName: String,
    inner: Symbol,
    runtimeConfig: RuntimeConfig,
) = RuntimeType.forInlineFun(newtypeName, ClientRustModule.config) {
    val codegenScope =
        arrayOf(
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
fun loadFromConfigBag(
    innerTypeName: String,
    newtype: RuntimeType,
): Writable =
    writable {
        rustTemplate(
            """
            load::<#{newtype}>().map(#{f})
            """,
            "newtype" to newtype,
            "f" to
                writable {
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
fun standardConfigParam(param: ConfigParam): ConfigCustomization =
    object : ConfigCustomization() {
        override fun section(section: ServiceConfig): Writable {
            return when (section) {
                ServiceConfig.BuilderImpl ->
                    writable {
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
                        rustTemplate(
                            """
                            pub fn set_${param.name}(&mut self, ${param.name}: Option<#{T}>) -> &mut Self {
                                self.config.store_or_unset(${param.name}.map(#{newtype}));
                                self
                            }
                            """,
                            "T" to param.type,
                            "newtype" to param.newtype!!,
                        )
                    }

                is ServiceConfig.BuilderFromConfigBag ->
                    writable {
                        rustTemplate(
                            """
                            ${section.builder}.set_${param.name}(${section.configBag}.#{load_from_config_bag});
                            """,
                            "load_from_config_bag" to loadFromConfigBag(param.type.name, param.newtype!!),
                        )
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
            return ServiceConfigGenerator(codegenContext, baseFeatures + extraCustomizations)
        }
    }

    private val moduleUseName = codegenContext.moduleUseName()
    private val runtimeConfig = codegenContext.runtimeConfig
    private val enableUserConfigurableRuntimePlugins = codegenContext.enableUserConfigurableRuntimePlugins
    private val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)
    val codegenScope =
        arrayOf(
            *preludeScope,
            "BoxError" to RuntimeType.boxError(runtimeConfig),
            "CloneableLayer" to smithyTypes.resolve("config_bag::CloneableLayer"),
            "ConfigBag" to RuntimeType.configBag(codegenContext.runtimeConfig),
            "Cow" to RuntimeType.Cow,
            "FrozenLayer" to configReexport(smithyTypes.resolve("config_bag::FrozenLayer")),
            "Layer" to configReexport(smithyTypes.resolve("config_bag::Layer")),
            "Resolver" to RuntimeType.smithyRuntime(runtimeConfig).resolve("client::config_override::Resolver"),
            "RuntimeComponentsBuilder" to configReexport(RuntimeType.runtimeComponentsBuilder(runtimeConfig)),
            "RuntimePlugin" to configReexport(RuntimeType.runtimePlugin(runtimeConfig)),
            "SharedRuntimePlugin" to configReexport(RuntimeType.sharedRuntimePlugin(runtimeConfig)),
            "runtime_plugin" to RuntimeType.smithyRuntimeApiClient(runtimeConfig).resolve("client::runtime_plugin"),
            "BehaviorVersion" to
                configReexport(
                    RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("client::behavior_version::BehaviorVersion"),
                ),
        )

    private fun builderFromConfigBag() =
        writable {
            val builderVar = "builder"
            val configBagVar = "config_bag"

            docs("Constructs a config builder from the given `$configBagVar`, setting only fields stored in the config bag,")
            docs("but not those in runtime components.")
            Attribute.AllowUnused.render(this)
            rustBlockTemplate(
                "pub(crate) fn from_config_bag($configBagVar: &#{ConfigBag}) -> Self",
                *codegenScope,
            ) {
                rust("let mut $builderVar = Self::new();")
                customizations.forEach {
                    it.section(ServiceConfig.BuilderFromConfigBag(builderVar, configBagVar))(this)
                }
                rust(builderVar)
            }
        }

    private fun behaviorMv() =
        writable {
            val docs = """
                /// Sets the [`behavior major version`](crate::config::BehaviorVersion).
                ///
                /// Over time, new best-practice behaviors are introduced. However, these behaviors might not be backwards
                /// compatible. For example, a change which introduces new default timeouts or a new retry-mode for
                /// all operations might be the ideal behavior but could break existing applications.
                ///
                /// ## Examples
                ///
                /// Set the behavior major version to `latest`. This is equivalent to enabling the `behavior-version-latest` cargo feature.
                /// ```no_run
                /// use $moduleUseName::config::BehaviorVersion;
                ///
                /// let config = $moduleUseName::Config::builder()
                ///     .behavior_version(BehaviorVersion::latest())
                ///     // ...
                ///     .build();
                /// let client = $moduleUseName::Client::from_conf(config);
                /// ```
                ///
                /// Customizing behavior major version:
                /// ```no_run
                /// use $moduleUseName::config::BehaviorVersion;
                ///
                /// let config = $moduleUseName::Config::builder()
                ///     .behavior_version(BehaviorVersion::v2023_11_09())
                ///     // ...
                ///     .build();
                /// let client = $moduleUseName::Client::from_conf(config);
                /// ```
            ///"""
            rustTemplate(
                """
                $docs
                pub fn behavior_version(mut self, behavior_version: crate::config::BehaviorVersion) -> Self {
                    self.set_behavior_version(Some(behavior_version));
                    self
                }

                $docs
                pub fn set_behavior_version(&mut self, behavior_version: Option<crate::config::BehaviorVersion>) -> &mut Self {
                    self.behavior_version = behavior_version;
                    self
                }

                /// Convenience method to set the latest behavior major version
                ///
                /// This is equivalent to enabling the `behavior-version-latest` Cargo feature
                pub fn behavior_version_latest(mut self) -> Self {
                    self.set_behavior_version(Some(crate::config::BehaviorVersion::latest()));
                    self
                }
                """,
                *codegenScope,
            )
        }

    fun render(writer: RustWriter) {
        val configDocs = """
            Constructs a config builder.
            <div class="warning">
            Note that a config created from this builder will not have the same safe defaults as one created by
            the <a href="https://crates.io/crates/aws-config" target="_blank">aws-config</a> crate.
            </div>
        """

        // The doc line directly following this attribute is sometimes followed by empty lines.
        // This triggers a clippy lint. I cannot figure out what is adding those lines, so
        // allowing the line for now. Setting it as an inner attribute since this happens several
        // times in this file.
        Attribute.AllowClippyEmptyLineAfterDocComments.render(writer, AttributeKind.Inner)
        writer.docs("Configuration for a $moduleUseName service client.\n")
        customizations.forEach {
            it.section(ServiceConfig.ConfigStructAdditionalDocs)(writer)
        }
        Attribute(Attribute.derive(RuntimeType.Clone, RuntimeType.Debug)).render(writer)
        writer.rustBlock("pub struct Config") {
            rustTemplate(
                """
                // Both `config` and `cloneable` are the same config, but the cloneable one
                // is kept around so that it is possible to convert back into a builder. This can be
                // optimized in the future.
                pub(crate) config: #{FrozenLayer},
                cloneable: #{CloneableLayer},
                pub(crate) runtime_components: #{RuntimeComponentsBuilder},
                pub(crate) runtime_plugins: #{Vec}<#{SharedRuntimePlugin}>,
                pub(crate) behavior_version: #{Option}<#{BehaviorVersion}>,
                """,
                *codegenScope,
            )
            customizations.forEach {
                it.section(ServiceConfig.ConfigStruct)(this)
            }
        }

        writer.rustBlock("impl Config") {
            writer.docs(configDocs)
            writer.rustTemplate(
                """
                pub fn builder() -> Builder { Builder::default() }
                """,
            )
            writer.rustTemplate(
                """
                /// Converts this config back into a builder so that it can be tweaked.
                pub fn to_builder(&self) -> Builder {
                    Builder {
                        config: self.cloneable.clone(),
                        runtime_components: self.runtime_components.clone(),
                        runtime_plugins: self.runtime_plugins.clone(),
                        behavior_version: self.behavior_version,
                    }
                }
                """,
            )
            customizations.forEach {
                it.section(ServiceConfig.ConfigImpl)(this)
            }
        }

        writer.docs("Builder for creating a `Config`.")
        Attribute(Attribute.derive(RuntimeType.Clone, RuntimeType.Debug)).render(writer)
        writer.rustBlock("pub struct Builder") {
            rustTemplate(
                """
                pub(crate) config: #{CloneableLayer},
                pub(crate) runtime_components: #{RuntimeComponentsBuilder},
                pub(crate) runtime_plugins: #{Vec}<#{SharedRuntimePlugin}>,
                pub(crate) behavior_version: #{Option}<#{BehaviorVersion}>,
                """,
                *codegenScope,
            )
            customizations.forEach {
                it.section(ServiceConfig.BuilderStruct)(this)
            }
        }

        // Custom implementation of Default to give the runtime components builder a name
        writer.rustBlockTemplate("impl #{Default} for Builder", *codegenScope) {
            writer.rustTemplate(
                """
                fn default() -> Self {
                    Self {
                        config: #{Default}::default(),
                        runtime_components: #{RuntimeComponentsBuilder}::new("service config"),
                        runtime_plugins: #{Default}::default(),
                        behavior_version: #{Default}::default(),
                    }
                }
                """,
                *codegenScope,
            )
        }

        writer.rustBlock("impl Builder") {
            writer.docs(configDocs)
            writer.rust("pub fn new() -> Self { Self::default() }")

            builderFromConfigBag()(this)
            customizations.forEach {
                it.section(ServiceConfig.BuilderImpl)(this)
            }
            behaviorMv()(this)

            val visibility =
                if (enableUserConfigurableRuntimePlugins) {
                    "pub"
                } else {
                    "pub(crate)"
                }

            docs("Adds a runtime plugin to the config.")
            if (!enableUserConfigurableRuntimePlugins) {
                Attribute.AllowUnused.render(this)
            }
            rustTemplate(
                """
                $visibility fn runtime_plugin(mut self, plugin: impl #{RuntimePlugin} + 'static) -> Self {
                    self.push_runtime_plugin(#{SharedRuntimePlugin}::new(plugin));
                    self
                }
                """,
                *codegenScope,
            )
            docs("Adds a runtime plugin to the config.")
            if (!enableUserConfigurableRuntimePlugins) {
                Attribute.AllowUnused.render(this)
            }
            rustTemplate(
                """
                $visibility fn push_runtime_plugin(&mut self, plugin: #{SharedRuntimePlugin}) -> &mut Self {
                    self.runtime_plugins.push(plugin);
                    self
                }
                """,
                *codegenScope,
            )

            val testUtilOnly =
                Attribute(Attribute.cfg(Attribute.any(Attribute.feature(TestUtilFeature.name), writable("test"))))

            testUtilOnly.render(this)
            Attribute.AllowUnusedMut.render(this)
            docs("Apply test defaults to the builder. NOTE: Consider migrating to use `apply_test_defaults_v2` instead.")
            rustBlock("pub fn apply_test_defaults(&mut self) -> &mut Self") {
                customizations.forEach { it.section(ServiceConfig.DefaultForTests("self"))(this) }
                rustTemplate(
                    "self.behavior_version = #{Some}(crate::config::BehaviorVersion::latest());",
                    *preludeScope,
                )
                rust("self")
            }

            testUtilOnly.render(this)
            Attribute.AllowUnusedMut.render(this)
            docs("Apply test defaults to the builder. NOTE: Consider migrating to use `with_test_defaults_v2` instead.")
            rustBlock("pub fn with_test_defaults(mut self) -> Self") {
                rust("self.apply_test_defaults(); self")
            }

            testUtilOnly.render(this)
            Attribute.AllowUnusedMut.render(this)
            docs("Apply test defaults to the builder. V2 of this function sets additional test defaults such as region configuration (if applicable).")
            rustBlock("pub fn apply_test_defaults_v2(&mut self) -> &mut Self") {
                rust("self.apply_test_defaults();")
                customizations.forEach { it.section(ServiceConfig.DefaultForTestsV2("self"))(this) }
                rust("self")
            }

            testUtilOnly.render(this)
            Attribute.AllowUnusedMut.render(this)
            docs("Apply test defaults to the builder. V2 of this function sets additional test defaults such as region configuration (if applicable).")
            rustBlock("pub fn with_test_defaults_v2(mut self) -> Self") {
                rust("self.apply_test_defaults_v2(); self")
            }

            docs("Builds a [`Config`].")
            rust("##[allow(unused_mut)]")
            rustBlock("pub fn build(mut self) -> Config") {
                rustTemplate(
                    """
                    let mut layer = self.config;
                    """,
                    *codegenScope,
                )
                customizations.forEach {
                    it.section(ServiceConfig.BuilderBuild)(this)
                }
                rustBlock("Config") {
                    rustTemplate(
                        """
                        config: #{Layer}::from(layer.clone()).with_name("$moduleUseName::config::Config").freeze(),
                        cloneable: layer,
                        runtime_components: self.runtime_components,
                        runtime_plugins: self.runtime_plugins,
                        behavior_version: self.behavior_version,
                        """,
                        *codegenScope,
                    )
                }
            }

            customizations.forEach {
                it.section(ServiceConfig.Extras)(writer)
            }
        }
    }
}
