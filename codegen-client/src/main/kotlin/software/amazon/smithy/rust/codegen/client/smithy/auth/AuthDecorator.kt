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
 * * Adds setters to the service config builder for configuring the auth scheme and auth scheme resolver
 * * Adds getters to the service config for retrieving the currently configured auth scheme and auth scheme resolver
 */
class AuthDecorator : ClientCodegenDecorator {
    override val name: String = "Auth"
    override val order: Byte = ORDER

    companion object {
        const val ORDER: Byte = 0
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        baseCustomizations + AuthDecoratorConfigCustomizations(codegenContext) + AuthSchemePreferenceConfigCustomization(codegenContext)

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
                                section.registerAuthSchemeOptionResolver(
                                    this,
                                    defaultAuthSchemeResolver(codegenContext),
                                )
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
    val codegenScope =
        arrayOf(
            *preludeScope,
            "AuthScheme" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig).resolve("client::auth::AuthScheme"),
            "NoAuthRuntimePluginV2" to
                RuntimeType.smithyRuntime(codegenContext.runtimeConfig).resolve("client::auth::no_auth::NoAuthRuntimePluginV2"),
            "ResolveAuthSchemeOptions" to AuthTypesGenerator(codegenContext).serviceSpecificResolveAuthSchemeTrait(),
            "SharedAuthScheme" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig).resolve("client::auth::SharedAuthScheme"),
            "SharedAuthSchemeOptionResolver" to RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig).resolve("client::auth::SharedAuthSchemeOptionResolver"),
        )

    override fun section(section: ServiceConfig) =
        writable {
            val moduleUseName = codegenContext.moduleUseName()
            when (section) {
                is ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Adds an auth scheme to the builder
                        ///
                        /// If `auth_scheme` has an existing [AuthSchemeId](aws_smithy_runtime_api::client::auth::AuthSchemeId) in the runtime, the current identity
                        /// resolver and signer for that scheme will be replaced by those from `auth_scheme`.
                        ///
                        /// _Important:_ When introducing a custom auth scheme, ensure you override either
                        /// [`Self::auth_scheme_resolver`] or [`Self::set_auth_scheme_resolver`]
                        /// so that the custom auth scheme is included in the list of resolved auth scheme options.
                        /// [The default auth scheme resolver](crate::config::auth::DefaultAuthSchemeResolver) will not recognize your custom auth scheme.
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## use aws_smithy_runtime_api::{
                        /// ##     box_error::BoxError,
                        /// ##     client::{
                        /// ##         auth::{
                        /// ##             AuthScheme, AuthSchemeEndpointConfig, AuthSchemeId, AuthSchemeOption,
                        /// ##             AuthSchemeOptionsFuture, Sign,
                        /// ##         },
                        /// ##         identity::{Identity, IdentityFuture, ResolveIdentity, SharedIdentityResolver},
                        /// ##         orchestrator::HttpRequest,
                        /// ##         runtime_components::{GetIdentityResolver, RuntimeComponents},
                        /// ##   },
                        /// ##   shared::IntoShared,
                        /// ## };
                        /// ## use aws_smithy_types::config_bag::ConfigBag;
                        /// // Auth scheme with customer identity resolver and signer
                        /// ##[derive(Debug)]
                        /// struct CustomAuthScheme {
                        ///     id: AuthSchemeId,
                        ///     identity_resolver: SharedIdentityResolver,
                        ///     signer: CustomSigner,
                        /// }
                        /// impl Default for CustomAuthScheme {
                        ///     fn default() -> Self {
                        ///         Self {
                        ///             id: AuthSchemeId::new("custom"),
                        ///             identity_resolver: CustomIdentityResolver.into_shared(),
                        ///             signer: CustomSigner,
                        ///         }
                        ///     }
                        /// }
                        /// impl AuthScheme for CustomAuthScheme {
                        ///     fn scheme_id(&self) -> AuthSchemeId {
                        ///         self.id.clone()
                        ///     }
                        ///     fn identity_resolver(
                        ///         &self,
                        ///         _identity_resolvers: &dyn GetIdentityResolver,
                        ///     ) -> Option<SharedIdentityResolver> {
                        ///         Some(self.identity_resolver.clone())
                        ///     }
                        ///     fn signer(&self) -> &dyn Sign {
                        ///         &self.signer
                        ///     }
                        /// }
                        ///
                        /// ##[derive(Debug, Default)]
                        /// struct CustomSigner;
                        /// impl Sign for CustomSigner {
                        ///     fn sign_http_request(
                        ///         &self,
                        ///         _request: &mut HttpRequest,
                        ///         _identity: &Identity,
                        ///         _auth_scheme_endpoint_config: AuthSchemeEndpointConfig<'_>,
                        ///         _runtime_components: &RuntimeComponents,
                        ///         _config_bag: &ConfigBag,
                        ///     ) -> Result<(), BoxError> {
                        ///         // --snip--
                        /// ##      todo!()
                        ///     }
                        /// }
                        ///
                        /// ##[derive(Debug)]
                        /// struct CustomIdentityResolver;
                        /// impl ResolveIdentity for CustomIdentityResolver {
                        ///     fn resolve_identity<'a>(
                        ///         &'a self,
                        ///         _runtime_components: &'a RuntimeComponents,
                        ///         _config_bag: &'a ConfigBag,
                        ///     ) -> IdentityFuture<'a> {
                        ///         // --snip--
                        /// ##      todo!()
                        ///     }
                        /// }
                        ///
                        /// // Auth scheme resolver that favors `CustomAuthScheme`
                        /// ##[derive(Debug)]
                        /// struct CustomAuthSchemeResolver;
                        /// impl $moduleUseName::config::auth::ResolveAuthScheme for CustomAuthSchemeResolver {
                        ///     fn resolve_auth_scheme<'a>(
                        ///         &'a self,
                        ///         _params: &'a $moduleUseName::config::auth::Params,
                        ///         _cfg: &'a ConfigBag,
                        ///         _runtime_components: &'a RuntimeComponents,
                        ///     ) -> AuthSchemeOptionsFuture<'a> {
                        ///         AuthSchemeOptionsFuture::ready(Ok(vec![AuthSchemeOption::from(AuthSchemeId::new(
                        ///             "custom",
                        ///         ))]))
                        ///     }
                        /// }
                        ///
                        /// let config = $moduleUseName::Config::builder()
                        ///     .push_auth_scheme(CustomAuthScheme::default())
                        ///     .auth_scheme_resolver(CustomAuthSchemeResolver)
                        ///     // other configurations
                        ///     .build();
                        /// ```
                        pub fn push_auth_scheme(mut self, auth_scheme: impl #{AuthScheme} + 'static) -> Self {
                            self.runtime_components.push_auth_scheme(auth_scheme);
                            self
                        }

                        /// Set the auth scheme resolver for the builder
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// ## use aws_smithy_runtime_api::{
                        /// ##     client::{
                        /// ##         auth::AuthSchemeOptionsFuture,
                        /// ##         runtime_components::RuntimeComponents,
                        /// ##   },
                        /// ## };
                        /// ## use aws_smithy_types::config_bag::ConfigBag;
                        /// ##[derive(Debug)]
                        /// struct CustomAuthSchemeResolver;
                        /// impl $moduleUseName::config::auth::ResolveAuthScheme for CustomAuthSchemeResolver {
                        ///     fn resolve_auth_scheme<'a>(
                        ///         &'a self,
                        ///         _params: &'a $moduleUseName::config::auth::Params,
                        ///         _cfg: &'a ConfigBag,
                        ///         _runtime_components: &'a RuntimeComponents,
                        ///     ) -> AuthSchemeOptionsFuture<'a> {
                        ///         // --snip--
                        /// ##      todo!()
                        ///     }
                        /// }
                        ///
                        /// let config = $moduleUseName::Config::builder()
                        ///     .auth_scheme_resolver(CustomAuthSchemeResolver)
                        ///     // other configurations
                        ///     .build();
                        /// ```
                        pub fn auth_scheme_resolver(mut self, auth_scheme_resolver: impl #{ResolveAuthSchemeOptions} + 'static) -> Self {
                            self.set_auth_scheme_resolver(auth_scheme_resolver);
                            self
                        }

                        /// Set the auth scheme resolver for the builder
                        ///
                        /// ## Examples
                        /// See an example for [`Self::auth_scheme_resolver`].
                        pub fn set_auth_scheme_resolver(&mut self, auth_scheme_resolver: impl #{ResolveAuthSchemeOptions} + 'static) -> &mut Self {
                            self.runtime_components.set_auth_scheme_option_resolver(#{Some}(auth_scheme_resolver.into_shared_resolver()));
                            self
                        }

                        /// Add [NoAuthScheme](aws_smithy_runtime::client::auth::no_auth::NoAuthScheme) as a fallback for operations that don't require authentication
                        ///
                        /// The auth scheme resolver will use this when no other auth schemes are applicable.
                        pub fn allow_no_auth(mut self) -> Self {
                            self.set_allow_no_auth();
                            self
                        }

                        /// Add [NoAuthScheme](aws_smithy_runtime::client::auth::no_auth::NoAuthScheme) as a fallback for operations that don't require authentication
                        ///
                        /// The auth scheme resolver will use this when no other auth schemes are applicable.
                        pub fn set_allow_no_auth(&mut self) -> &mut Self {
                            self.push_runtime_plugin(#{NoAuthRuntimePluginV2}::new().into_shared());
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Return the auth schemes configured on this service config
                        pub fn auth_schemes(&self) -> impl Iterator<Item = #{SharedAuthScheme}> + '_ {
                            self.runtime_components.auth_schemes()
                        }

                        /// Return the auth scheme resolver configured on this service config
                        pub fn auth_scheme_resolver(&self) -> #{Option}<#{SharedAuthSchemeOptionResolver}> {
                            self.runtime_components.auth_scheme_option_resolver()
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
        }
}

private class AuthSchemePreferenceConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "AuthSchemePreference" to
                RuntimeType.smithyRuntimeApiClient(codegenContext.runtimeConfig)
                    .resolve("client::auth::AuthSchemePreference"),
        )
    private val moduleUseName = codegenContext.moduleUseName()

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Returns the configured auth scheme preference
                        pub fn auth_scheme_preference(&self) -> #{Option}<&#{AuthSchemePreference}> {
                            self.config.load::<#{AuthSchemePreference}>()
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderImpl -> {
                    val docs = """
                        /// Set the auth scheme preference for an auth scheme resolver
                        /// (typically the default auth scheme resolver).
                        ///
                        /// Each operation has a predefined order of auth schemes, as determined by the service,
                        /// for auth scheme resolution. By using the auth scheme preference, customers
                        /// can reorder the schemes resolved by the auth scheme resolver.
                        ///
                        /// The preference list is intended as a hint rather than a strict override.
                        /// Any schemes not present in the originally resolved auth schemes will be ignored.
                        ///
                        /// ## Examples
                        ///
                        /// ```no_run
                        /// ## use aws_smithy_runtime_api::client::auth::AuthSchemeId;
                        /// let config = $moduleUseName::Config::builder()
                        ///     .auth_scheme_preference([AuthSchemeId::from("scheme1"), AuthSchemeId::from("scheme2")])
                        ///     // ...
                        ///     .build();
                        /// let client = $moduleUseName::Client::from_conf(config);
                        /// ```
                    """
                    rustTemplate(
                        """
                        $docs
                        pub fn auth_scheme_preference(mut self, preference: impl #{Into}<#{AuthSchemePreference}>) -> Self {
                            self.set_auth_scheme_preference(#{Some}(preference.into()));
                            self
                        }

                        $docs
                        pub fn set_auth_scheme_preference(&mut self, preference: #{Option}<#{AuthSchemePreference}>) -> &mut Self {
                            self.config.store_or_unset(preference);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderFromConfigBag ->
                    rustTemplate(
                        "${section.builder}.set_auth_scheme_preference(${section.configBag}.load::<#{AuthSchemePreference}>().cloned());",
                        *codegenScope,
                    )

                else -> emptySection
            }
        }
}
