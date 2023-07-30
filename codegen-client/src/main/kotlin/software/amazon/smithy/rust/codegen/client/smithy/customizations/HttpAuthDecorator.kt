/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpApiKeyAuthTrait
import software.amazon.smithy.model.traits.HttpBasicAuthTrait
import software.amazon.smithy.model.traits.HttpBearerAuthTrait
import software.amazon.smithy.model.traits.HttpDigestAuthTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.AuthSchemeOption.StaticAuthSchemeOption
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.letIf

private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> {
    val smithyRuntime =
        CargoDependency.smithyRuntime(runtimeConfig).withFeature("http-auth").toType()
    val smithyRuntimeApi = CargoDependency.smithyRuntimeApi(runtimeConfig).withFeature("http-auth").toType()
    val authHttp = smithyRuntime.resolve("client::auth::http")
    val authHttpApi = smithyRuntimeApi.resolve("client::auth::http")
    return arrayOf(
        "AuthSchemeId" to smithyRuntimeApi.resolve("client::auth::AuthSchemeId"),
        "ApiKeyAuthScheme" to authHttp.resolve("ApiKeyAuthScheme"),
        "ApiKeyLocation" to authHttp.resolve("ApiKeyLocation"),
        "BasicAuthScheme" to authHttp.resolve("BasicAuthScheme"),
        "BearerAuthScheme" to authHttp.resolve("BearerAuthScheme"),
        "DigestAuthScheme" to authHttp.resolve("DigestAuthScheme"),
        "HTTP_API_KEY_AUTH_SCHEME_ID" to authHttpApi.resolve("HTTP_API_KEY_AUTH_SCHEME_ID"),
        "HTTP_BASIC_AUTH_SCHEME_ID" to authHttpApi.resolve("HTTP_BASIC_AUTH_SCHEME_ID"),
        "HTTP_BEARER_AUTH_SCHEME_ID" to authHttpApi.resolve("HTTP_BEARER_AUTH_SCHEME_ID"),
        "HTTP_DIGEST_AUTH_SCHEME_ID" to authHttpApi.resolve("HTTP_DIGEST_AUTH_SCHEME_ID"),
        "IdentityResolver" to smithyRuntimeApi.resolve("client::identity::IdentityResolver"),
        "Login" to smithyRuntimeApi.resolve("client::identity::http::Login"),
        "PropertyBag" to RuntimeType.smithyHttp(runtimeConfig).resolve("property_bag::PropertyBag"),
        "SharedAuthScheme" to smithyRuntimeApi.resolve("client::auth::SharedAuthScheme"),
        "SharedIdentityResolver" to smithyRuntimeApi.resolve("client::identity::SharedIdentityResolver"),
        "Token" to smithyRuntimeApi.resolve("client::identity::http::Token"),
    )
}

private data class HttpAuthSchemes(
    val apiKey: Boolean,
    val basic: Boolean,
    val bearer: Boolean,
    val digest: Boolean,
) {
    companion object {
        fun from(codegenContext: ClientCodegenContext): HttpAuthSchemes {
            val authSchemes = ServiceIndex.of(codegenContext.model).getAuthSchemes(codegenContext.serviceShape).keys
            val generateOrchestrator = codegenContext.smithyRuntimeMode.generateOrchestrator
            return HttpAuthSchemes(
                apiKey = generateOrchestrator && authSchemes.contains(HttpApiKeyAuthTrait.ID),
                basic = generateOrchestrator && authSchemes.contains(HttpBasicAuthTrait.ID),
                bearer = generateOrchestrator && authSchemes.contains(HttpBearerAuthTrait.ID),
                digest = generateOrchestrator && authSchemes.contains(HttpDigestAuthTrait.ID),
            )
        }
    }

    fun anyEnabled(): Boolean = isTokenBased() || isLoginBased()
    fun isTokenBased(): Boolean = apiKey || bearer
    fun isLoginBased(): Boolean = basic || digest
}

class HttpAuthDecorator : ClientCodegenDecorator {
    override val name: String get() = "HttpAuthDecorator"
    override val order: Byte = 0

    override fun authOptions(
        codegenContext: ClientCodegenContext,
        operationShape: OperationShape,
        baseAuthSchemeOptions: List<AuthSchemeOption>,
    ): List<AuthSchemeOption> {
        val serviceIndex = ServiceIndex.of(codegenContext.model)
        val authSchemes = serviceIndex.getEffectiveAuthSchemes(codegenContext.serviceShape, operationShape)
        val codegenScope = codegenScope(codegenContext.runtimeConfig)
        val options = ArrayList<AuthSchemeOption>()
        for (authScheme in authSchemes.keys) {
            fun addOption(schemeShapeId: ShapeId, name: String) {
                options.add(
                    StaticAuthSchemeOption(
                        schemeShapeId,
                        writable {
                            rustTemplate("$name,", *codegenScope)
                        },
                    ),
                )
            }
            when (authScheme) {
                HttpApiKeyAuthTrait.ID -> addOption(authScheme, "#{HTTP_API_KEY_AUTH_SCHEME_ID}")
                HttpBasicAuthTrait.ID -> addOption(authScheme, "#{HTTP_BASIC_AUTH_SCHEME_ID}")
                HttpBearerAuthTrait.ID -> addOption(authScheme, "#{HTTP_BEARER_AUTH_SCHEME_ID}")
                HttpDigestAuthTrait.ID -> addOption(authScheme, "#{HTTP_DIGEST_AUTH_SCHEME_ID}")
                else -> {}
            }
        }
        return baseAuthSchemeOptions + options
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> =
        HttpAuthSchemes.from(codegenContext).let { authSchemes ->
            baseCustomizations.letIf(authSchemes.anyEnabled()) {
                it + HttpAuthConfigCustomization(codegenContext, authSchemes)
            }
        }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> =
        HttpAuthSchemes.from(codegenContext).let { authSchemes ->
            baseCustomizations.letIf(authSchemes.anyEnabled()) {
                it + HttpAuthServiceRuntimePluginCustomization(codegenContext, authSchemes)
            }
        }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val authSchemes = HttpAuthSchemes.from(codegenContext)
        if (authSchemes.anyEnabled()) {
            rustCrate.withModule(ClientRustModule.config) {
                val codegenScope = codegenScope(codegenContext.runtimeConfig)
                if (authSchemes.isTokenBased()) {
                    rustTemplate("pub use #{Token};", *codegenScope)
                }
                if (authSchemes.isLoginBased()) {
                    rustTemplate("pub use #{Login};", *codegenScope)
                }
            }
        }
    }
}

private class HttpAuthServiceRuntimePluginCustomization(
    private val codegenContext: ClientCodegenContext,
    private val authSchemes: HttpAuthSchemes,
) : ServiceRuntimePluginCustomization() {
    private val serviceShape = codegenContext.serviceShape
    private val codegenScope = codegenScope(codegenContext.runtimeConfig)

    override fun section(section: ServiceRuntimePluginSection): Writable = writable {
        when (section) {
            is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                fun registerAuthScheme(scheme: Writable) {
                    section.registerAuthScheme(this) {
                        rustTemplate("#{SharedAuthScheme}::new(#{Scheme})", *codegenScope, "Scheme" to scheme)
                    }
                }
                fun registerNamedAuthScheme(name: String) {
                    registerAuthScheme {
                        rustTemplate("#{$name}::new()", *codegenScope)
                    }
                }

                if (authSchemes.apiKey) {
                    val trait = serviceShape.getTrait<HttpApiKeyAuthTrait>()!!
                    val location = when (trait.`in`!!) {
                        HttpApiKeyAuthTrait.Location.HEADER -> {
                            check(trait.scheme.isPresent) {
                                "A scheme is required for `@httpApiKey` when `in` is set to `header`"
                            }
                            "Header"
                        }

                        HttpApiKeyAuthTrait.Location.QUERY -> "Query"
                    }

                    registerAuthScheme {
                        rustTemplate(
                            """
                            #{ApiKeyAuthScheme}::new(
                                ${trait.scheme.orElse("").dq()},
                                #{ApiKeyLocation}::$location,
                                ${trait.name.dq()},
                            )
                            """,
                            *codegenScope,
                        )
                    }
                }
                if (authSchemes.basic) {
                    registerNamedAuthScheme("BasicAuthScheme")
                }
                if (authSchemes.bearer) {
                    registerNamedAuthScheme("BearerAuthScheme")
                }
                if (authSchemes.digest) {
                    registerNamedAuthScheme("DigestAuthScheme")
                }
            }

            else -> emptySection
        }
    }
}

private class HttpAuthConfigCustomization(
    codegenContext: ClientCodegenContext,
    private val authSchemes: HttpAuthSchemes,
) : ConfigCustomization() {
    private val codegenScope = codegenScope(codegenContext.runtimeConfig)

    override fun section(section: ServiceConfig): Writable = writable {
        when (section) {
            is ServiceConfig.BuilderImpl -> {
                if (authSchemes.apiKey) {
                    rustTemplate(
                        """
                        /// Sets the API key that will be used for authentication.
                        pub fn api_key(self, api_key: #{Token}) -> Self {
                            self.api_key_resolver(api_key)
                        }

                        /// Sets an API key resolver will be used for authentication.
                        pub fn api_key_resolver(mut self, api_key_resolver: impl #{IdentityResolver} + 'static) -> Self {
                            self.runtime_components.push_identity_resolver(
                                #{HTTP_API_KEY_AUTH_SCHEME_ID},
                                #{SharedIdentityResolver}::new(api_key_resolver)
                            );
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
                if (authSchemes.bearer) {
                    rustTemplate(
                        """
                        /// Sets the bearer token that will be used for HTTP bearer auth.
                        pub fn bearer_token(self, bearer_token: #{Token}) -> Self {
                            self.bearer_token_resolver(bearer_token)
                        }

                        /// Sets a bearer token provider that will be used for HTTP bearer auth.
                        pub fn bearer_token_resolver(mut self, bearer_token_resolver: impl #{IdentityResolver} + 'static) -> Self {
                            self.runtime_components.push_identity_resolver(
                                #{HTTP_BEARER_AUTH_SCHEME_ID},
                                #{SharedIdentityResolver}::new(bearer_token_resolver)
                            );
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
                if (authSchemes.basic) {
                    rustTemplate(
                        """
                        /// Sets the login that will be used for HTTP basic auth.
                        pub fn basic_auth_login(self, basic_auth_login: #{Login}) -> Self {
                            self.basic_auth_login_resolver(basic_auth_login)
                        }

                        /// Sets a login resolver that will be used for HTTP basic auth.
                        pub fn basic_auth_login_resolver(mut self, basic_auth_resolver: impl #{IdentityResolver} + 'static) -> Self {
                            self.runtime_components.push_identity_resolver(
                                #{HTTP_BASIC_AUTH_SCHEME_ID},
                                #{SharedIdentityResolver}::new(basic_auth_resolver)
                            );
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
                if (authSchemes.digest) {
                    rustTemplate(
                        """
                        /// Sets the login that will be used for HTTP digest auth.
                        pub fn digest_auth_login(self, digest_auth_login: #{Login}) -> Self {
                            self.digest_auth_login_resolver(digest_auth_login)
                        }

                        /// Sets a login resolver that will be used for HTTP digest auth.
                        pub fn digest_auth_login_resolver(mut self, digest_auth_resolver: impl #{IdentityResolver} + 'static) -> Self {
                            self.runtime_components.push_identity_resolver(
                                #{HTTP_DIGEST_AUTH_SCHEME_ID},
                                #{SharedIdentityResolver}::new(digest_auth_resolver)
                            );
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            else -> {}
        }
    }
}
