/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.HttpApiKeyAuthTrait
import software.amazon.smithy.model.traits.OptionalAuthTrait
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.Trait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.core.util.letIf

/**
 * Inserts a ApiKeyAuth configuration into the operation
 */
class ApiKeyAuthDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "ApiKeyAuth"
    override val order: Byte = 10

    private fun applies(codegenContext: ClientCodegenContext) =
        isSupportedApiKeyAuth(codegenContext)

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.letIf(applies(codegenContext)) { customizations ->
            customizations + ApiKeyConfigCustomization(codegenContext.runtimeConfig)
        }
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations.letIf(applies(codegenContext)) { customizations ->
            customizations + ApiKeyPubUse(codegenContext.runtimeConfig)
        }
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        if (applies(codegenContext) && hasApiKeyAuthScheme(codegenContext, operation)) {
            val service = codegenContext.serviceShape
            val authDefinition = mutableMapOf<String, Any>()
            authDefinition.put("in", service.expectTrait(HttpApiKeyAuthTrait::class.java).getIn().toString())
            authDefinition.put("name", service.expectTrait(HttpApiKeyAuthTrait::class.java).getName())
            service.expectTrait(HttpApiKeyAuthTrait::class.java).getScheme().ifPresent { scheme ->
                authDefinition.put("scheme", scheme)
            }
            return baseCustomizations + ApiKeyOperationCustomization(codegenContext.runtimeConfig, authDefinition)
        }
        return baseCustomizations
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

/**
 * Returns if the service supports the httpApiKeyAuth trait.
 *
 * @param codegenContext Codegen context that includes the model and service shape
 * @return if the httpApiKeyAuth trait is used by the service
 */
fun isSupportedApiKeyAuth(codegenContext: ClientCodegenContext): Boolean {
    return ServiceIndex.of(codegenContext.model).getAuthSchemes(codegenContext.serviceShape).containsKey(HttpApiKeyAuthTrait.ID);
}

/**
 * Returns if the service and operation have the httpApiKeyAuthTrait.
 *
 * @param codegenContext codegen context that includes the model and service shape
 * @param operation operation shape
 * @return if the service and operation have the httpApiKeyAuthTrait
 */
fun hasApiKeyAuthScheme(codegenContext: ClientCodegenContext, operation: OperationShape): Boolean {
    val auth: Map<ShapeId, Trait> = ServiceIndex.of(codegenContext.model).getEffectiveAuthSchemes(codegenContext.serviceShape.getId(), operation.getId());
    return auth.containsKey(HttpApiKeyAuthTrait.ID) && !operation.hasTrait(OptionalAuthTrait.ID);
}

private class ApiKeyPubUse(private val runtimeConfig: RuntimeConfig) :
    LibRsCustomization() {
    override fun section(section: LibRsSection): Writable = when (section) {
        is LibRsSection.Body -> writable {
            rust(
                "pub use #T;",
                apiKey(runtimeConfig),
            )
        }
        else -> emptySection
    }
}

private class ApiKeyOperationCustomization(private val runtimeConfig: RuntimeConfig, private val authDefinition: Map<String, Any>) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            rustBlock("if let Some(api_key_config) = ${section.config}.api_key()") {
                rust("""
                    ${section.request}.properties_mut().insert(api_key_config.clone());
                    let api_key = api_key_config.api_key();
                """)
                val definitionName = authDefinition.get("name") as String
                if (authDefinition.get("in") == "query") {
                    rustTemplate(
                        """
                        let auth_definition = #{http_auth_definition}::new_with_query(
                            "${definitionName}".to_owned(),
                        ).expect("valid definition for api key auth");
                        let name = auth_definition.name();
                        let mut query = #{query_writer}::new(${section.request}.http().uri());
                        query.insert(name, api_key);
                        *${section.request}.http_mut().uri_mut() = query.build_uri();
                        """,
                        "http_auth_definition" to
                        RuntimeType.smithyTypes(runtimeConfig).resolve("auth::HttpAuthDefinition"),
                        "definition_name" to authDefinition.get("name") as String,
                        "query_writer" to RuntimeType.smithyHttp(runtimeConfig).resolve("query_writer::QueryWriter"),
                    )
                } else {
                    var definitionScheme = authDefinition.get("scheme") as String?
                    if (definitionScheme != null) {
                        definitionScheme = "Some(\"" + definitionScheme + "\".to_owned())"
                    } else {
                        definitionScheme = "None"
                    }
                    rustTemplate(
                        """
                        let auth_definition = #{http_auth_definition}::new_with_header(
                            "${definitionName}".to_owned(),
                            ${definitionScheme},
                        ).expect("valid definition for api key auth");
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
                        RuntimeType.smithyTypes(runtimeConfig).resolve("auth::HttpAuthDefinition"),
                        "http_header" to RuntimeType.Http.resolve("header"),
                    )
                }
                
            }
        }
        else -> emptySection
    }
}

private class ApiKeyConfigCustomization(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope = arrayOf(
        "ApiKey" to apiKey(runtimeConfig),
    )

    override fun section(section: ServiceConfig): Writable =
        when (section) {
            is ServiceConfig.BuilderStruct -> writable {
                rustTemplate("api_key: Option<#{ApiKey}>,", *codegenScope)
            }
            is ServiceConfig.BuilderImpl -> writable {
                rustTemplate(
                    """
                    /// Sets the api key that will be used by the client.
                    pub fn api_key(mut self, api_key: #{ApiKey}) -> Self {
                        self.set_api_key(Some(api_key));
                        self
                    }

                    /// Sets the api key that will be used by the client.
                    pub fn set_api_key(&mut self, api_key: Option<#{ApiKey}>) -> &mut Self {
                        self.api_key = api_key;
                        self
                    }
                    """,
                    *codegenScope,
                )
            }
            is ServiceConfig.BuilderBuild -> writable {
                rust("api_key: self.api_key,")
            }
            is ServiceConfig.ConfigStruct -> writable {
                rustTemplate("api_key: Option<#{ApiKey}>,", *codegenScope)
            }
            is ServiceConfig.ConfigImpl -> writable {
                rustTemplate(
                    """
                    /// Returns api key used by the client, if it was provided.
                    pub fn api_key(&self) -> Option<&#{ApiKey}> {
                        self.api_key.as_ref()
                    }
                    """,
                    *codegenScope,
                )
            }
            else -> emptySection
        }
}

fun apiKey(runtimeConfig: RuntimeConfig) = RuntimeType.smithyTypes(runtimeConfig).resolve("auth::AuthApiKey")
