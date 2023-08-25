/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization

class CredentialsCacheDecorator : ClientCodegenDecorator {
    override val name: String = "CredentialsCache"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + CredentialCacheConfig(codegenContext)
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust("${section.serviceConfigBuilder}.set_credentials_cache(${section.sdkConfig}.credentials_cache().cloned());")
            },
        )
}

/**
 * Add a `.credentials_cache` field and builder to the `Config` for a given service
 */
class CredentialCacheConfig(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope = arrayOf(
        *preludeScope,
        "CredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("cache::CredentialsCache"),
        "DefaultProvider" to defaultProvider(),
        "SharedAsyncSleep" to RuntimeType.smithyAsync(runtimeConfig).resolve("rt::sleep::SharedAsyncSleep"),
        "SharedCredentialsCache" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("cache::SharedCredentialsCache"),
        "SharedCredentialsProvider" to AwsRuntimeType.awsCredentialTypes(runtimeConfig).resolve("provider::SharedCredentialsProvider"),
    )

    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.ConfigImpl -> {
                rustTemplate(
                    """
                    /// Returns the credentials cache.
                    pub fn credentials_cache(&self) -> #{Option}<#{SharedCredentialsCache}> {
                        self.config.load::<#{SharedCredentialsCache}>().cloned()
                    }
                    """,
                    *codegenScope,
                )
            }

            ServiceConfig.BuilderImpl -> {
                rustTemplate(
                    """
                    /// Sets the credentials cache for this service
                    pub fn credentials_cache(mut self, credentials_cache: #{CredentialsCache}) -> Self {
                        self.set_credentials_cache(#{Some}(credentials_cache));
                        self
                    }

                    """,
                    *codegenScope,
                )

                rustTemplate(
                    """
                    /// Sets the credentials cache for this service
                    pub fn set_credentials_cache(&mut self, credentials_cache: #{Option}<#{CredentialsCache}>) -> &mut Self {
                        self.config.store_or_unset(credentials_cache);
                        self
                    }
                    """,
                    *codegenScope,
                )
            }

            ServiceConfig.BuilderBuild -> {
                rustTemplate(
                    """
                    if let Some(credentials_provider) = layer.load::<#{SharedCredentialsProvider}>().cloned() {
                        let cache_config = layer.load::<#{CredentialsCache}>().cloned()
                            .unwrap_or_else({
                                let sleep = self.runtime_components.sleep_impl();
                                || match sleep {
                                    Some(sleep) => {
                                        #{CredentialsCache}::lazy_builder()
                                            .sleep(sleep)
                                            .into_credentials_cache()
                                    }
                                    None => #{CredentialsCache}::lazy(),
                                }
                            });
                        let shared_credentials_cache = cache_config.create_cache(credentials_provider);
                        layer.store_put(shared_credentials_cache);
                    }
                    """,
                    *codegenScope,
                )
            }

            is ServiceConfig.OperationConfigOverride -> {
                rustTemplate(
                    """
                    match (
                        resolver.config_mut().load::<#{CredentialsCache}>().cloned(),
                        resolver.config_mut().load::<#{SharedCredentialsProvider}>().cloned(),
                    ) {
                        (#{None}, #{None}) => {}
                        (#{None}, _) => {
                            panic!("also specify `.credentials_cache` when overriding credentials provider for the operation");
                        }
                        (_, #{None}) => {
                            panic!("also specify `.credentials_provider` when overriding credentials cache for the operation");
                        }
                        (
                            #{Some}(credentials_cache),
                            #{Some}(credentials_provider),
                        ) => {
                            resolver.config_mut().store_put(credentials_cache.create_cache(credentials_provider));
                        }
                    }
                    """,
                    *codegenScope,
                )
            }

            else -> emptySection
        }
    }
}
