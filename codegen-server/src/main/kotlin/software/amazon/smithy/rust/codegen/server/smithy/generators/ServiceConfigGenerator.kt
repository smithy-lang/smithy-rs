/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.join
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.toPascalCase
import software.amazon.smithy.rust.codegen.server.smithy.ServerCargoDependency
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext

fun List<ConfigMethod>.isBuilderFallible() = this.any { it.isRequired }

/**
 * Contains all data necessary to render a method on the config builder object to apply arbitrary layers, HTTP plugins,
 * and model plugins.
 */
data class ConfigMethod(
    /** The name of the method. **/
    val name: String,
    /** The Rust docs for the method. **/
    val docs: String,
    /** The parameters of the method. **/
    val params: List<Binding>,
    /** In case the method is fallible, the error type it returns. **/
    val errorType: RuntimeType?,
    /** The code block inside the method. **/
    val initializer: Initializer,
    /** Whether the user must invoke the method or not. **/
    val isRequired: Boolean,
) {
    /** The name of the flag on the config builder object that tracks whether the _required_ method has already been invoked or not. **/
    fun requiredBuilderFlagName(): String {
        check(isRequired) {
            "Config method is not required so it shouldn't need a field in the builder tracking whether it has been configured"
        }
        return "${name}_configured"
    }

    /** The name of the enum variant on the config builder's error struct for a _required_ method. **/
    fun requiredErrorVariant(): String {
        check(isRequired) {
            "Config method is not required so it shouldn't need an error variant"
        }
        return "${name.toPascalCase()}NotConfigured"
    }
}

/**
 * Represents the code block inside the method that initializes and configures a set of layers, HTTP plugins, and/or model
 * plugins.
 */
data class Initializer(
    /**
     * The code itself that initializes and configures the layers, HTTP plugins, and/or model plugins. This should be
     * a set of [Rust statements] that, after execution, defines one variable binding per layer/HTTP plugin/model plugin
     * that it has configured and wants to apply. The code may use the method's input arguments (see [params] in
     * [ConfigMethod]) to perform checks and initialize the bindings.
     *
     * For example, the following code performs checks on the `authorizer` and `auth_spec` input arguments, returning
     * an error (see [errorType] in [ConfigMethod]) in case these checks fail, and leaves two plugins defined in two
     * variable bindings, `authn_plugin` and `authz_plugin`.
     *
     * ```rust
     * if authorizer != 69 {
     *     return Err(std::io::Error::new(std::io::ErrorKind::Other, "failure 1"));
     * }

     * if auth_spec.len() != 69 {
     *     return Err(std::io::Error::new(std::io::ErrorKind::Other, "failure 2"));
     * }
     * let authn_plugin = #{SmithyHttpServer}::plugin::IdentityPlugin;
     * let authz_plugin = #{SmithyHttpServer}::plugin::IdentityPlugin;
     * ```
     *
     * [Rust statements]: https://doc.rust-lang.org/reference/statements.html
     */
    val code: Writable,
    /** Ordered list of layers that should be applied. Layers are executed in the order they appear in the list. **/
    val layerBindings: List<Binding>,
    /** Ordered list of HTTP plugins that should be applied. Http plugins are executed in the order they appear in the list. **/
    val httpPluginBindings: List<Binding>,
    /** Ordered list of model plugins that should be applied. Model plugins are executed in the order they appear in the list. **/
    val modelPluginBindings: List<Binding>,
)

/**
 * Represents a variable binding. For example, the following Rust code:
 *
 * ```rust
 * fn foo(bar: String) {
 *     let baz: u64 = 69;
 * }
 *
 * has two variable bindings. The `bar` name is bound to a `String` variable and the `baz` name is bound to a
 * `u64` variable.
 * ```
 */
data class Binding(
    /** The name of the variable. */
    val name: String,
    /** The type of the variable. */
    val ty: RuntimeType,
)

class ServiceConfigGenerator(
    codegenContext: ServerCodegenContext,
    private val configMethods: List<ConfigMethod>,
) {
    private val crateName = codegenContext.moduleUseName()
    private val smithyHttpServer = ServerCargoDependency.smithyHttpServer(codegenContext.runtimeConfig).toType()
    private val codegenScope = arrayOf(
        *preludeScope,
        "Debug" to RuntimeType.Debug,
        "SmithyHttpServer" to smithyHttpServer,
        "PluginStack" to smithyHttpServer.resolve("plugin::PluginStack"),
        "ModelMarker" to smithyHttpServer.resolve("plugin::ModelMarker"),
        "HttpMarker" to smithyHttpServer.resolve("plugin::HttpMarker"),
        "Tower" to RuntimeType.Tower,
        "Stack" to RuntimeType.Tower.resolve("layer::util::Stack"),
    )
    private val serviceName = codegenContext.serviceShape.id.name.toPascalCase()

    fun render(writer: RustWriter) {
        val unwrapConfigBuilder = if (isBuilderFallible) {
            """
            ///    .expect("config failed to build");
            """
        } else {
            ";"
        }

        writer.rustTemplate(
            """
            /// Configuration for the [`$serviceName`]. This is the central place where to register and
            /// configure [`#{Tower}::Layer`]s, HTTP plugins, and model plugins.
            ///
            /// ```rust,no_run
            /// ## use $crateName::${serviceName}Config;
            /// ## use #{SmithyHttpServer}::plugin::IdentityPlugin;
            /// ## use #{Tower}::layer::util::Identity;
            /// ## let authentication_plugin = IdentityPlugin;
            /// ## let authorization_plugin = IdentityPlugin;
            /// ## let server_request_id_provider_layer = Identity::new();
            /// let config = ${serviceName}Config::builder()
            ///     // Layers get executed first...
            ///     .layer(server_request_id_provider_layer)
            ///     // ...then HTTP plugins...
            ///     .http_plugin(authentication_plugin)
            ///     // ...and right after deserialization, model plugins.
            ///     .model_plugin(authorization_plugin)
            ///     .build()$unwrapConfigBuilder
            /// ```
            ///
            /// See the [`plugin`] system for details.
            ///
            /// [`plugin`]: #{SmithyHttpServer}::plugin
            ##[derive(#{Debug})]
            pub struct ${serviceName}Config<L, H, M> {
                layers: L,
                http_plugins: H,
                model_plugins: M,
            }

            impl ${serviceName}Config<(), (), ()> {
                /// Returns a builder to construct the configuration.
                pub fn builder() -> ${serviceName}ConfigBuilder<
                    #{Tower}::layer::util::Identity,
                    #{SmithyHttpServer}::plugin::IdentityPlugin,
                    #{SmithyHttpServer}::plugin::IdentityPlugin,
                > {
                    ${serviceName}ConfigBuilder {
                        layers: #{Tower}::layer::util::Identity::new(),
                        http_plugins: #{SmithyHttpServer}::plugin::IdentityPlugin,
                        model_plugins: #{SmithyHttpServer}::plugin::IdentityPlugin,
                        #{BuilderRequiredMethodFlagsInit:W}
                    }
                }
            }

            /// Builder returned by [`${serviceName}Config::builder()`].
            ##[derive(#{Debug})]
            pub struct ${serviceName}ConfigBuilder<L, H, M> {
                pub(crate) layers: L,
                pub(crate) http_plugins: H,
                pub(crate) model_plugins: M,
                #{BuilderRequiredMethodFlagDefinitions:W}
            }
            
            #{BuilderRequiredMethodError:W}

            impl<L, H, M> ${serviceName}ConfigBuilder<L, H, M> {
                #{InjectedMethods:W}
            
                /// Add a [`#{Tower}::Layer`] to the service.
                pub fn layer<NewLayer>(self, layer: NewLayer) -> ${serviceName}ConfigBuilder<#{Stack}<NewLayer, L>, H, M> {
                    ${serviceName}ConfigBuilder {
                        layers: #{Stack}::new(layer, self.layers),
                        http_plugins: self.http_plugins,
                        model_plugins: self.model_plugins,
                        #{BuilderRequiredMethodFlagsMove1:W}
                    }
                }

                /// Add a HTTP [plugin] to the service.
                ///
                /// [plugin]: #{SmithyHttpServer}::plugin
                // We eagerly require `NewPlugin: HttpMarker`, despite not really needing it, because compiler
                // errors get _substantially_ better if the user makes a mistake.
                pub fn http_plugin<NewPlugin: #{HttpMarker}>(
                    self,
                    http_plugin: NewPlugin,
                ) -> ${serviceName}ConfigBuilder<L, #{PluginStack}<NewPlugin, H>, M> {
                    ${serviceName}ConfigBuilder {
                        layers: self.layers,
                        http_plugins: #{PluginStack}::new(http_plugin, self.http_plugins),
                        model_plugins: self.model_plugins,
                        #{BuilderRequiredMethodFlagsMove2:W}
                    }
                }

                /// Add a model [plugin] to the service.
                ///
                /// [plugin]: #{SmithyHttpServer}::plugin
                // We eagerly require `NewPlugin: ModelMarker`, despite not really needing it, because compiler
                // errors get _substantially_ better if the user makes a mistake.
                pub fn model_plugin<NewPlugin: #{ModelMarker}>(
                    self,
                    model_plugin: NewPlugin,
                ) -> ${serviceName}ConfigBuilder<L, H, #{PluginStack}<NewPlugin, M>> {
                    ${serviceName}ConfigBuilder {
                        layers: self.layers,
                        http_plugins: self.http_plugins,
                        model_plugins: #{PluginStack}::new(model_plugin, self.model_plugins),
                        #{BuilderRequiredMethodFlagsMove3:W}
                    }
                }
                
                #{BuilderBuildMethod:W}
            }
            """,
            *codegenScope,
            "BuilderRequiredMethodFlagsInit" to builderRequiredMethodFlagsInit(),
            "BuilderRequiredMethodFlagDefinitions" to builderRequiredMethodFlagsDefinitions(),
            "BuilderRequiredMethodError" to builderRequiredMethodError(),
            "InjectedMethods" to injectedMethods(),
            "BuilderRequiredMethodFlagsMove1" to builderRequiredMethodFlagsMove(),
            "BuilderRequiredMethodFlagsMove2" to builderRequiredMethodFlagsMove(),
            "BuilderRequiredMethodFlagsMove3" to builderRequiredMethodFlagsMove(),
            "BuilderBuildMethod" to builderBuildMethod(),
        )
    }

    private val isBuilderFallible = configMethods.isBuilderFallible()

    private fun builderBuildRequiredMethodChecks() = configMethods.filter { it.isRequired }.map {
        writable {
            rustTemplate(
                """
                if !self.${it.requiredBuilderFlagName()} {
                    return #{Err}(${serviceName}ConfigError::${it.requiredErrorVariant()});
                }
                """,
                *codegenScope,
            )
        }
    }.join("\n")

    private fun builderRequiredMethodFlagsDefinitions() = configMethods.filter { it.isRequired }.map {
        writable { rust("pub(crate) ${it.requiredBuilderFlagName()}: bool,") }
    }.join("\n")

    private fun builderRequiredMethodFlagsInit() = configMethods.filter { it.isRequired }.map {
        writable { rust("${it.requiredBuilderFlagName()}: false,") }
    }.join("\n")

    private fun builderRequiredMethodFlagsMove() = configMethods.filter { it.isRequired }.map {
        writable { rust("${it.requiredBuilderFlagName()}: self.${it.requiredBuilderFlagName()},") }
    }.join("\n")

    private fun builderRequiredMethodError() = writable {
        if (isBuilderFallible) {
            val variants = configMethods.filter { it.isRequired }.map {
                writable {
                    rust(
                        """
                    ##[error("service is not fully configured; invoke `${it.name}` on the config builder")]
                    ${it.requiredErrorVariant()},
                    """,
                    )
                }
            }
            rustTemplate(
                """
                ##[derive(Debug, #{ThisError}::Error)]
                pub enum ${serviceName}ConfigError {
                    #{Variants:W}
                }
                """,
                "ThisError" to ServerCargoDependency.ThisError.toType(),
                "Variants" to variants.join("\n"),
            )
        }
    }

    private fun injectedMethods() = configMethods.map {
        writable {
            val paramBindings = it.params.map { binding ->
                writable { rustTemplate("${binding.name}: #{BindingTy},", "BindingTy" to binding.ty) }
            }.join("\n")

            // This produces a nested type like: "S<B, S<A, T>>", where
            // - "S" denotes a "stack type" with two generic type parameters: the first is the "inner" part of the stack
            //   and the second is the "outer" part  of the stack. The outer part gets executed first. For an example,
            //   see `aws_smithy_http_server::plugin::PluginStack`.
            // - "A", "B" are the types of the "things" that are added.
            // - "T" is the generic type variable name used in the enclosing impl block.
            fun List<Binding>.stackReturnType(genericTypeVarName: String, stackType: RuntimeType): Writable =
                this.fold(writable { rust(genericTypeVarName) }) { acc, next ->
                    writable {
                        rustTemplate(
                            "#{StackType}<#{Ty}, #{Acc:W}>",
                            "StackType" to stackType,
                            "Ty" to next.ty,
                            "Acc" to acc,
                        )
                    }
                }

            val layersReturnTy =
                it.initializer.layerBindings.stackReturnType("L", RuntimeType.Tower.resolve("layer::util::Stack"))
            val httpPluginsReturnTy =
                it.initializer.httpPluginBindings.stackReturnType("H", smithyHttpServer.resolve("plugin::PluginStack"))
            val modelPluginsReturnTy =
                it.initializer.modelPluginBindings.stackReturnType("M", smithyHttpServer.resolve("plugin::PluginStack"))

            val configBuilderReturnTy = writable {
                rustTemplate(
                    """
                    ${serviceName}ConfigBuilder<
                        #{LayersReturnTy:W},
                        #{HttpPluginsReturnTy:W},
                        #{ModelPluginsReturnTy:W},
                    >
                    """,
                    "LayersReturnTy" to layersReturnTy,
                    "HttpPluginsReturnTy" to httpPluginsReturnTy,
                    "ModelPluginsReturnTy" to modelPluginsReturnTy,
                )
            }

            val returnTy = if (it.errorType != null) {
                writable {
                    rustTemplate(
                        "#{Result}<#{T:W}, #{E}>",
                        "T" to configBuilderReturnTy,
                        "E" to it.errorType,
                        *codegenScope,
                    )
                }
            } else {
                configBuilderReturnTy
            }

            docs(it.docs)
            rustBlockTemplate(
                """
                pub fn ${it.name}(
                    ##[allow(unused_mut)]
                    mut self,
                    #{ParamBindings:W}
                ) -> #{ReturnTy:W}
                """,
                "ReturnTy" to returnTy,
                "ParamBindings" to paramBindings,
            ) {
                rustTemplate("#{InitializerCode:W}", "InitializerCode" to it.initializer.code)

                check(it.initializer.layerBindings.size + it.initializer.httpPluginBindings.size + it.initializer.modelPluginBindings.size > 0) {
                    "This method's initializer does not register any layers, HTTP plugins, or model plugins. It must register at least something!"
                }

                if (it.isRequired) {
                    rust("self.${it.requiredBuilderFlagName()} = true;")
                }
                conditionalBlock("Ok(", ")", conditional = it.errorType != null) {
                    val registrations = (
                        it.initializer.layerBindings.map { ".layer(${it.name})" } +
                            it.initializer.httpPluginBindings.map { ".http_plugin(${it.name})" } +
                            it.initializer.modelPluginBindings.map { ".model_plugin(${it.name})" }
                        ).joinToString("")
                    rust("self$registrations")
                }
            }
        }
    }.join("\n\n")

    private fun builderBuildReturnType() = writable {
        val t = "super::${serviceName}Config<L, H, M>"

        if (isBuilderFallible) {
            rustTemplate("#{Result}<$t, ${serviceName}ConfigError>", *codegenScope)
        } else {
            rust(t)
        }
    }

    private fun builderBuildMethod() = writable {
        rustBlockTemplate(
            """
            /// Build the configuration.
            pub fn build(self) -> #{BuilderBuildReturnTy:W}
            """,
            "BuilderBuildReturnTy" to builderBuildReturnType(),
        ) {
            rustTemplate(
                "#{BuilderBuildRequiredMethodChecks:W}",
                "BuilderBuildRequiredMethodChecks" to builderBuildRequiredMethodChecks(),
            )

            conditionalBlock("Ok(", ")", isBuilderFallible) {
                rust(
                    """
                    super::${serviceName}Config {
                        layers: self.layers,
                        http_plugins: self.http_plugins,
                        model_plugins: self.model_plugins,
                    }
                    """,
                )
            }
        }
    }
}
