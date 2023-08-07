/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
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

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + CredentialsCacheFeature(codegenContext.runtimeConfig)
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
    private val runtimeMode = codegenContext.smithyRuntimeMode
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
            ServiceConfig.ConfigStruct -> {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate(
                        """pub(crate) credentials_cache: #{SharedCredentialsCache},""",
                        *codegenScope,
                    )
                }
            }

            ServiceConfig.ConfigImpl -> {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Returns the credentials cache.
                        pub fn credentials_cache(&self) -> #{Option}<#{SharedCredentialsCache}> {
                            self.config.load::<#{SharedCredentialsCache}>().cloned()
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        /// Returns the credentials cache.
                        pub fn credentials_cache(&self) -> #{SharedCredentialsCache} {
                            self.credentials_cache.clone()
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            ServiceConfig.BuilderStruct ->
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("credentials_cache: #{Option}<#{CredentialsCache}>,", *codegenScope)
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

                if (runtimeMode.generateOrchestrator) {
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
                } else {
                    rustTemplate(
                        """
                        /// Sets the credentials cache for this service
                        pub fn set_credentials_cache(&mut self, credentials_cache: Option<#{CredentialsCache}>) -> &mut Self {
                            self.credentials_cache = credentials_cache;
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            ServiceConfig.BuilderBuild -> {
                if (runtimeMode.generateOrchestrator) {
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
                } else {
                    rustTemplate(
                        """
                        credentials_cache: self
                            .credentials_cache
                            .unwrap_or_else({
                                let sleep = self.sleep_impl.clone();
                                || match sleep {
                                    Some(sleep) => {
                                        #{CredentialsCache}::lazy_builder()
                                            .sleep(sleep)
                                            .into_credentials_cache()
                                    }
                                    None => #{CredentialsCache}::lazy(),
                                }
                            })
                            .create_cache(
                                self.credentials_provider.unwrap_or_else(|| {
                                    #{SharedCredentialsProvider}::new(#{DefaultProvider})
                                })
                            ),
                        """,
                        *codegenScope,
                    )
                }
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

class CredentialsCacheFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                rust(
                    """
                    #T(&mut ${section.request}.properties_mut(), ${section.config}.credentials_cache.clone());
                    """,
                    setCredentialsCache(runtimeConfig),
                )
            }

            else -> emptySection
        }
    }
}

fun setCredentialsCache(runtimeConfig: RuntimeConfig) =
    AwsRuntimeType.awsHttp(runtimeConfig).resolve("auth::set_credentials_cache")
