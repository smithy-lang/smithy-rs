/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.auth

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.NamedCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.Section

/**
 * An interface that provides information relevant to `AuthSchemeOption` during code generation
 *
 * * The shape ID of auth scheme ID
 * * A method that renders itself as a type `AuthSchemeOption` in Rust
 */
interface AuthSchemeOption {
    val authSchemeId: ShapeId

    fun render(
        codegenContext: ClientCodegenContext,
        operation: OperationShape? = null,
    ): Writable
}

/**
 * Auth-related code generation customization sections
 */
sealed class AuthSection(name: String) : Section(name) {
    /**
     * Hook to add additional logic within the implementation of the
     * `crate::config::auth::ResolveAuthScheme::resolve_auth_scheme` method for the default auth scheme resolver
     */
    data object DefaultResolverAdditionalImpl : AuthSection("DefaultResolverAdditionalImpl")
}

typealias AuthCustomization = NamedCustomization<AuthSection>

/**
 * Codegen decorator that:
 * * Injects service-specific default auth scheme resolver
 * * TODO(AuthAlignment): Expand this list as we add more customizations
 */
class AuthDecorator : ClientCodegenDecorator {
    override val name: String = "Auth"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + AuthDecoratorConfigCustomizations(codegenContext)

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> {
        return baseCustomizations +
            object : ServiceRuntimePluginCustomization() {
                override fun section(section: ServiceRuntimePluginSection): Writable {
                    return when (section) {
                        is ServiceRuntimePluginSection.RegisterRuntimeComponents ->
                            writable {
                                section.registerAuthSchemeOptionResolver(this, defaultAuthSchemeResolver(codegenContext))
                            }

                        else -> emptySection
                    }
                }
            }
    }
}

// Returns default auth scheme resolver for this service
private fun defaultAuthSchemeResolver(codegenContext: ClientCodegenContext): Writable {
    val generator = AuthTypesGenerator(codegenContext)
    return writable {
        rustTemplate(
            """{
            use #{ServiceSpecificResolver};
            #{DefaultResolver}::default().into_shared_resolver()
            }""",
            "DefaultResolver" to generator.defaultAuthSchemeResolver(),
            "ServiceSpecificResolver" to generator.serviceSpecificResolveAuthSchemeTrait(),
        )
    }
}

private class AuthDecoratorConfigCustomizations(private val codegenContext: ClientCodegenContext) :
    ConfigCustomization() {
    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Set the auth scheme option resolver for the builder
                        pub fn auth_scheme_option_resolver(mut self, auth_scheme_option_resolver: impl #{ResolveAuthSchemeOptions} + 'static) -> Self {
                            self.set_auth_scheme_option_resolver(auth_scheme_option_resolver);
                            self
                        }

                        /// Set the auth scheme option resolver for the builder
                        pub fn set_auth_scheme_option_resolver(&mut self, auth_scheme_option_resolver: impl #{ResolveAuthSchemeOptions} + 'static) -> &mut Self {
                            self.runtime_components.set_auth_scheme_option_resolver(#{Some}(auth_scheme_option_resolver.into_shared_resolver()));
                            self
                        }
                        """,
                        *preludeScope,
                        "ResolveAuthSchemeOptions" to AuthTypesGenerator(codegenContext).serviceSpecificResolveAuthSchemeTrait(),
                    )
                }

                is ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Return the auth scheme option resolver configured on this service config
                        pub fn auth_scheme_option_resolver(&self) -> #{Option}<#{SharedAuthSchemeOptionResolver}> {
                            self.runtime_components.auth_scheme_option_resolver()
                        }
                        """,
                        *preludeScope,
                        "SharedAuthSchemeOptionResolver" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig).resolve("client::auth::SharedAuthSchemeOptionResolver"),
                    )
                }

                else -> emptySection
            }
        }
}
