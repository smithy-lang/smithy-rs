/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.HttpApiKeyAuthTrait
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.util.letIf

// TODO(enableNewSmithyRuntimeCleanup): Delete this decorator when switching to the orchestrator

/**
 * Inserts a ApiKeyAuth configuration into the operation
 */
class ApiKeyAuthDecorator : ClientCodegenDecorator {
    override val name: String = "ApiKeyAuth"
    override val order: Byte = 10

    private fun applies(codegenContext: ClientCodegenContext) =
        codegenContext.smithyRuntimeMode.generateMiddleware &&
            isSupportedApiKeyAuth(codegenContext)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.letIf(applies(codegenContext)) { customizations ->
            customizations + ApiKeyConfigCustomization(codegenContext)
        }
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        if (applies(codegenContext) && hasApiKeyAuthScheme(codegenContext, operation)) {
            val service = codegenContext.serviceShape
            val authDefinition: HttpApiKeyAuthTrait = service.expectTrait(HttpApiKeyAuthTrait::class.java)
            return baseCustomizations + ApiKeyOperationCustomization(codegenContext.runtimeConfig, authDefinition)
        }
        return baseCustomizations
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        if (applies(codegenContext)) {
            rustCrate.withModule(ClientRustModule.config) {
                rust("pub use #T;", apiKey(codegenContext.runtimeConfig))
            }
        }
    }
}

/**
 * Returns if the service supports the httpApiKeyAuth trait.
 *
 * @param codegenContext Codegen context that includes the model and service shape
 * @return if the httpApiKeyAuth trait is used by the service
 */
private fun isSupportedApiKeyAuth(codegenContext: ClientCodegenContext): Boolean {
    return ServiceIndex.of(codegenContext.model).getAuthSchemes(codegenContext.serviceShape).containsKey(HttpApiKeyAuthTrait.ID)
}

/**
 * Returns if the service and operation have the httpApiKeyAuthTrait.
 *
 * @param codegenContext codegen context that includes the model and service shape
 * @param operation operation shape
 * @return if the service and operation have the httpApiKeyAuthTrait
 */
private fun hasApiKeyAuthScheme(codegenContext: ClientCodegenContext, operation: OperationShape): Boolean {
    val auth: Map<ShapeId, Trait> = ServiceIndex.of(codegenContext.model).getEffectiveAuthSchemes(codegenContext.serviceShape.getId(), operation.getId())
    return auth.containsKey(HttpApiKeyAuthTrait.ID) && !operation.hasTrait(OptionalAuthTrait.ID)
}

private class ApiKeyOperationCustomization(private val runtimeConfig: RuntimeConfig, private val authDefinition: HttpApiKeyAuthTrait) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            rustBlock("if let Some(api_key_config) = ${section.config}.api_key()") {
                rust(
                    """
                    ${section.request}.properties_mut().insert(api_key_config.clone());
                    let api_key = api_key_config.api_key();
                    """,
                )
                val definitionName = authDefinition.getName()
                if (authDefinition.getIn() == HttpApiKeyAuthTrait.Location.QUERY) {
                    rustTemplate(
                        """
                        let auth_definition = #{http_auth_definition}::query(
                            "$definitionName".to_owned(),
                        );
                        let name = auth_definition.name();
                        let mut query = #{query_writer}::new(${section.request}.http().uri());
                        query.insert(name, api_key);
                        *${section.request}.http_mut().uri_mut() = query.build_uri();
                        """,
                        "http_auth_definition" to
                            RuntimeType.smithyHttpAuth(runtimeConfig).resolve("definition::HttpAuthDefinition"),
                        "query_writer" to RuntimeType.smithyHttp(runtimeConfig).resolve("query_writer::QueryWriter"),
                    )
                } else {
                    val definitionScheme: String = authDefinition.getScheme()
                        .map { scheme ->
                            "Some(\"" + scheme + "\".to_owned())"
                        }
                        .orElse("None")
                    rustTemplate(
                        """
                        let auth_definition = #{http_auth_definition}::header(
                            "$definitionName".to_owned(),
                            $definitionScheme,
                        );
                        let name = auth_definition.name();
                        let value = match auth_definition.scheme() {
                            Some(value) => format!("{value} {api_key}"),
                            None => api_key.to_owned(),
                        };
                        ${section.request}
                            .http_mut()
                            .headers_mut()
                            .insert(
                                #{http_header}::HeaderName::from_bytes(name.as_bytes()).expect("valid header name for api key auth"),
                                #{http_header}::HeaderValue::from_bytes(value.as_bytes()).expect("valid header value for api key auth")
                            );
                        """,
                        "http_auth_definition" to
                            RuntimeType.smithyHttpAuth(runtimeConfig).resolve("definition::HttpAuthDefinition"),
                        "http_header" to RuntimeType.Http.resolve("header"),
                    )
                }
            }
        }
        else -> emptySection
    }
}

private class ApiKeyConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    val runtimeMode = codegenContext.smithyRuntimeMode
    val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        *preludeScope,
        "ApiKey" to apiKey(runtimeConfig),
    )

    override fun section(section: ServiceConfig): Writable =
        when (section) {
            is ServiceConfig.BuilderStruct -> writable {
                rustTemplate("api_key: #{Option}<#{ApiKey}>,", *codegenScope)
            }
            is ServiceConfig.BuilderImpl -> writable {
                rustTemplate(
                    """
                    /// Sets the API key that will be used by the client.
                    pub fn api_key(mut self, api_key: #{ApiKey}) -> Self {
                        self.set_api_key(Some(api_key));
                        self
                    }

                    /// Sets the API key that will be used by the client.
                    pub fn set_api_key(&mut self, api_key: #{Option}<#{ApiKey}>) -> &mut Self {
                        self.api_key = api_key;
                        self
                    }
                    """,
                    *codegenScope,
                )
            }
            is ServiceConfig.BuilderBuild -> writable {
                if (runtimeMode.generateOrchestrator) {
                    rust("layer.store_or_unset(self.api_key);")
                } else {
                    rust("api_key: self.api_key,")
                }
            }
            is ServiceConfig.ConfigStruct -> writable {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("api_key: #{Option}<#{ApiKey}>,", *codegenScope)
                }
            }
            is ServiceConfig.ConfigImpl -> writable {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Returns API key used by the client, if it was provided.
                        pub fn api_key(&self) -> #{Option}<&#{ApiKey}> {
                            self.config.load::<#{ApiKey}>()
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        /// Returns API key used by the client, if it was provided.
                        pub fn api_key(&self) -> #{Option}<&#{ApiKey}> {
                            self.api_key.as_ref()
                        }
                        """,
                        *codegenScope,
                    )
                }
            }
            else -> emptySection
        }
}

private fun apiKey(runtimeConfig: RuntimeConfig) = RuntimeType.smithyHttpAuth(runtimeConfig).resolve("api_key::AuthApiKey")
