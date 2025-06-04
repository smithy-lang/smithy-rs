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
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
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
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.client.smithy.auth.AuthSchemeOption as AuthSchemeOptionV2

private fun codegenScope(runtimeConfig: RuntimeConfig): Array<Pair<String, Any>> {
    val smithyRuntime =
        CargoDependency.smithyRuntime(runtimeConfig).withFeature("http-auth").toType()
    val smithyRuntimeApi = CargoDependency.smithyRuntimeApiClient(runtimeConfig).withFeature("http-auth").toType()
    val authHttp = smithyRuntime.resolve("client::auth::http")
    val authHttpApi = smithyRuntimeApi.resolve("client::auth::http")
    return arrayOf(
        "AuthSchemeOption" to
            RuntimeType.smithyRuntimeApiClient(runtimeConfig)
                .resolve("client::auth::AuthSchemeOption"),
        "IntoShared" to RuntimeType.smithyRuntimeApi(runtimeConfig).resolve("shared::IntoShared"),
        "Token" to configReexport(smithyRuntimeApi.resolve("client::identity::http::Token")),
        "Login" to configReexport(smithyRuntimeApi.resolve("client::identity::http::Login")),
        "ResolveIdentity" to configReexport(smithyRuntimeApi.resolve("client::identity::ResolveIdentity")),
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
        "SharedAuthScheme" to smithyRuntimeApi.resolve("client::auth::SharedAuthScheme"),
        "SharedIdentityResolver" to smithyRuntimeApi.resolve("client::identity::SharedIdentityResolver"),
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
            return HttpAuthSchemes(
                apiKey = authSchemes.contains(HttpApiKeyAuthTrait.ID),
                basic = authSchemes.contains(HttpBasicAuthTrait.ID),
                bearer = authSchemes.contains(HttpBearerAuthTrait.ID),
                digest = authSchemes.contains(HttpDigestAuthTrait.ID),
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

    override fun authSchemeOptions(
        codegenContext: ClientCodegenContext,
        baseAuthSchemeOptions: List<AuthSchemeOptionV2>,
    ): List<AuthSchemeOptionV2> {
        val serviceIndex = ServiceIndex.of(codegenContext.model)
        val authSchemes = serviceIndex.getAuthSchemes(codegenContext.serviceShape)
        val options = ArrayList<AuthSchemeOptionV2>()
        for (authScheme in authSchemes.keys) {
            fun addOption(
                schemeShapeId: ShapeId,
                name: String,
            ) {
                options.add(
                    object : AuthSchemeOptionV2 {
                        override val authSchemeId = schemeShapeId

                        override fun render(
                            codegenContext: ClientCodegenContext,
                            operation: OperationShape?,
                        ) = writable {
                            rustTemplate(
                                """
                                #{AuthSchemeOption}::builder()
                                    .scheme_id($name)
                                    .build()
                                    .expect("required fields set")
                                """,
                                *codegenScope(codegenContext.runtimeConfig),
                            )
                        }
                    },
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
            fun addOption(
                schemeShapeId: ShapeId,
                name: String,
            ) {
                options.add(
                    StaticAuthSchemeOption(
                        schemeShapeId,
                        listOf(
                            writable {
                                rustTemplate(name, *codegenScope)
                            },
                        ),
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
}

private class HttpAuthServiceRuntimePluginCustomization(
    codegenContext: ClientCodegenContext,
    private val authSchemes: HttpAuthSchemes,
) : ServiceRuntimePluginCustomization() {
    private val serviceShape = codegenContext.serviceShape
    private val codegenScope = codegenScope(codegenContext.runtimeConfig)

    override fun section(section: ServiceRuntimePluginSection): Writable =
        writable {
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
                        val location =
                            when (trait.`in`!!) {
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

    override fun section(section: ServiceConfig): Writable =
        writable {
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
                            pub fn api_key_resolver(mut self, api_key_resolver: impl #{ResolveIdentity} + 'static) -> Self {
                                self.runtime_components.set_identity_resolver(
                                    #{HTTP_API_KEY_AUTH_SCHEME_ID},
                                    #{IntoShared}::<#{SharedIdentityResolver}>::into_shared(api_key_resolver)
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
                            pub fn bearer_token_resolver(mut self, bearer_token_resolver: impl #{ResolveIdentity} + 'static) -> Self {
                                self.runtime_components.set_identity_resolver(
                                    #{HTTP_BEARER_AUTH_SCHEME_ID},
                                    #{IntoShared}::<#{SharedIdentityResolver}>::into_shared(bearer_token_resolver)
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
                            pub fn basic_auth_login_resolver(mut self, basic_auth_resolver: impl #{ResolveIdentity} + 'static) -> Self {
                                self.runtime_components.set_identity_resolver(
                                    #{HTTP_BASIC_AUTH_SCHEME_ID},
                                    #{IntoShared}::<#{SharedIdentityResolver}>::into_shared(basic_auth_resolver)
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
                            pub fn digest_auth_login_resolver(mut self, digest_auth_resolver: impl #{ResolveIdentity} + 'static) -> Self {
                                self.runtime_components.set_identity_resolver(
                                    #{HTTP_DIGEST_AUTH_SCHEME_ID},
                                    #{IntoShared}::<#{SharedIdentityResolver}>::into_shared(digest_auth_resolver)
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
