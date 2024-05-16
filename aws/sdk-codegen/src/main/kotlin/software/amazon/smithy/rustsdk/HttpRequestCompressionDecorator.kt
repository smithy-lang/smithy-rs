/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.knowledge.TopDownIndex
import software.amazon.smithy.model.traits.RequestCompressionTrait
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
import software.amazon.smithy.rust.codegen.core.util.thenSingletonListOf

class HttpRequestCompressionDecorator : ClientCodegenDecorator {
    override val name: String = "HttpRequestCompression"
    override val order: Byte = 0

    private fun usesRequestCompression(codegenContext: ClientCodegenContext): Boolean {
        val index = TopDownIndex.of(codegenContext.model)
        val ops = index.getContainedOperations(codegenContext.serviceShape.id)
        return ops.any { it.hasTrait(RequestCompressionTrait.ID) }
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations +
            usesRequestCompression(codegenContext).thenSingletonListOf {
                HttpRequestCompressionConfigCustomization(codegenContext)
            }
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return usesRequestCompression(codegenContext).thenSingletonListOf {
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust(
                    """
                    ${section.serviceConfigBuilder} = ${section.serviceConfigBuilder}
                        .disable_request_compression(${section.sdkConfig}.disable_request_compression());
                    ${section.serviceConfigBuilder} = ${section.serviceConfigBuilder}
                        .request_min_compression_size_bytes(${section.sdkConfig}.request_min_compression_size_bytes());
                    """,
                )
            }
        }
    }
}

class HttpRequestCompressionConfigCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            "DisableRequestCompression" to RuntimeType.clientRequestCompression(runtimeConfig).resolve("DisableRequestCompression"),
            "RequestMinCompressionSizeBytes" to RuntimeType.clientRequestCompression(runtimeConfig).resolve("RequestMinCompressionSizeBytes"),
            "Storable" to RuntimeType.smithyTypes(runtimeConfig).resolve("config_bag::Storable"),
            "StoreReplace" to RuntimeType.smithyTypes(runtimeConfig).resolve("config_bag::StoreReplace"),
            *preludeScope,
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Returns the `disable request compression` setting, if it was provided.
                        pub fn disable_request_compression(&self) -> #{Option}<bool> {
                            self.config.load::<#{DisableRequestCompression}>().map(|it| it.0)
                        }

                        /// Returns the `request minimum compression size in bytes`, if it was provided.
                        pub fn request_min_compression_size_bytes(&self) -> #{Option}<u32> {
                            self.config.load::<#{RequestMinCompressionSizeBytes}>().map(|it| it.0)
                        }
                        """,
                        *codegenScope,
                    )
                }

                ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Sets the `disable request compression` used when making requests.
                        pub fn disable_request_compression(mut self, disable_request_compression: impl #{Into}<#{Option}<bool>>) -> Self {
                            self.set_disable_request_compression(disable_request_compression.into());
                            self
                        }

                        /// Sets the `request minimum compression size in bytes` used when making requests.
                        pub fn request_min_compression_size_bytes(mut self, request_min_compression_size_bytes: impl #{Into}<#{Option}<u32>>) -> Self {
                            self.set_request_min_compression_size_bytes(request_min_compression_size_bytes.into());
                            self
                        }
                        """,
                        *codegenScope,
                    )

                    rustTemplate(
                        """
                        /// Sets the `disable request compression` used when making requests.
                        pub fn set_disable_request_compression(&mut self, disable_request_compression: #{Option}<bool>) -> &mut Self {
                            self.config.store_or_unset::<#{DisableRequestCompression}>(disable_request_compression.map(Into::into));
                            self
                        }

                        /// Sets the `request minimum compression size in bytes` used when making requests.
                        pub fn set_request_min_compression_size_bytes(&mut self, request_min_compression_size_bytes: #{Option}<u32>) -> &mut Self {
                            self.config.store_or_unset::<#{RequestMinCompressionSizeBytes}>(request_min_compression_size_bytes.map(Into::into));
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderFromConfigBag -> {
                    rustTemplate(
                        """
                        ${section.builder}.set_disable_request_compression(
                            ${section.configBag}.load::<#{DisableRequestCompression}>().cloned().map(|it| it.0));
                        ${section.builder}.set_request_min_compression_size_bytes(
                            ${section.configBag}.load::<#{RequestMinCompressionSizeBytes}>().cloned().map(|it| it.0));
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
        }
}
