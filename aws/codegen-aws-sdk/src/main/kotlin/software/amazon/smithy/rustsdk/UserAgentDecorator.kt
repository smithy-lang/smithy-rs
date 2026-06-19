/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.ServiceRuntimePluginSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.CrateVersionCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait

/**
 * Inserts a UserAgent configuration into the operation
 */
class UserAgentDecorator : ClientCodegenDecorator {
    override val name: String = "UserAgent"
    override val order: Byte = 10

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + AppNameCustomization(codegenContext) + FrameworkMetadataCustomization(codegenContext)
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations + AddApiMetadataIntoConfigBag(codegenContext)

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust("${section.serviceConfigBuilder}.set_app_name(${section.sdkConfig}.app_name().cloned());")
            },
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                // Framework metadata is additive (each entry self-identifies a distinct library),
                // so copy every entry from the shared config rather than replacing.
                rust(
                    """
                    for framework_metadata in ${section.sdkConfig}.framework_metadata() {
                        ${section.serviceConfigBuilder}.push_framework_metadata(framework_metadata.clone());
                    }
                    """,
                )
            },
        )
    }

    /**
     * Adds a static `API_METADATA` variable to the crate `config` containing the serviceId & the version of the crate for this individual service
     */
    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val runtimeConfig = codegenContext.runtimeConfig

        // We are generating an AWS SDK, the service needs to have the AWS service trait
        val serviceTrait = codegenContext.serviceShape.expectTrait<ServiceTrait>()
        val serviceId = serviceTrait.sdkId.lowercase().replace(" ", "")

        rustCrate.withModule(ClientRustModule.Meta) {
            rustTemplate(
                """
                pub(crate) static API_METADATA: #{user_agent}::ApiMetadata =
                    #{user_agent}::ApiMetadata::new(${serviceId.dq()}, #{PKG_VERSION});
                """,
                "user_agent" to AwsRuntimeType.awsRuntime(runtimeConfig).resolve("user_agent"),
                "PKG_VERSION" to CrateVersionCustomization.pkgVersion(ClientRustModule.Meta),
            )
        }

        rustCrate.withModule(ClientRustModule.config) {
            // Re-export the app name so that it can be specified in config programmatically without an explicit dependency
            rustTemplate(
                "pub use #{AppName};",
                "AppName" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("app_name::AppName"),
            )
            // Re-export the framework metadata type so third-party libraries can self-identify in the
            // user agent without taking an explicit dependency on `aws-types`.
            rustTemplate(
                "pub use #{FrameworkMetadata};",
                "FrameworkMetadata" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("sdk_ua_metadata::FrameworkMetadata"),
            )
        }
    }

    private class AddApiMetadataIntoConfigBag(codegenContext: ClientCodegenContext) :
        ServiceRuntimePluginCustomization() {
        private val runtimeConfig = codegenContext.runtimeConfig
        private val awsRuntime = AwsRuntimeType.awsRuntime(runtimeConfig)

        override fun section(section: ServiceRuntimePluginSection): Writable =
            writable {
                when (section) {
                    is ServiceRuntimePluginSection.RegisterRuntimeComponents -> {
                        section.registerInterceptor(this) {
                            rust("#T::new()", awsRuntime.resolve("user_agent::UserAgentInterceptor"))
                        }
                    }
                    else -> emptySection
                }
            }
    }

    private class AppNameCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
        private val runtimeConfig = codegenContext.runtimeConfig
        private val codegenScope =
            arrayOf(
                *preludeScope,
                "AppName" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("app_name::AppName"),
                "AwsUserAgent" to AwsRuntimeType.awsRuntime(runtimeConfig).resolve("user_agent::AwsUserAgent"),
            )

        override fun section(section: ServiceConfig): Writable =
            when (section) {
                is ServiceConfig.BuilderImpl ->
                    writable {
                        rustTemplate(
                            """
                            /// Sets the name of the app that is using the client.
                            ///
                            /// This _optional_ name is used to identify the application in the user agent that
                            /// gets sent along with requests.
                            pub fn app_name(mut self, app_name: #{AppName}) -> Self {
                                self.set_app_name(Some(app_name));
                                self
                            }
                            """,
                            *codegenScope,
                        )

                        rustTemplate(
                            """
                            /// Sets the name of the app that is using the client.
                            ///
                            /// This _optional_ name is used to identify the application in the user agent that
                            /// gets sent along with requests.
                            pub fn set_app_name(&mut self, app_name: #{Option}<#{AppName}>) -> &mut Self {
                                self.config.store_or_unset(app_name);
                                self
                            }
                            """,
                            *codegenScope,
                        )
                    }

                is ServiceConfig.BuilderFromConfigBag ->
                    writable {
                        rustTemplate("${section.builder}.set_app_name(${section.configBag}.load::<#{AppName}>().cloned());", *codegenScope)
                    }

                is ServiceConfig.BuilderBuild ->
                    writable {
                        rust("layer.store_put(#T.clone());", ClientRustModule.Meta.toType().resolve("API_METADATA"))
                    }

                is ServiceConfig.ConfigImpl ->
                    writable {
                        rustTemplate(
                            """
                            /// Returns the name of the app that is using the client, if it was provided.
                            ///
                            /// This _optional_ name is used to identify the application in the user agent that
                            /// gets sent along with requests.
                            pub fn app_name(&self) -> #{Option}<&#{AppName}> {
                               self.config.load::<#{AppName}>()
                            }
                            """,
                            *codegenScope,
                        )
                    }

                is ServiceConfig.DefaultForTests ->
                    writable {
                        rustTemplate(
                            """
                            self.config.store_put(#{AwsUserAgent}::for_tests());
                            """,
                            *codegenScope,
                        )
                    }

                else -> emptySection
            }
    }

    private class FrameworkMetadataCustomization(codegenContext: ClientCodegenContext) : ConfigCustomization() {
        private val runtimeConfig = codegenContext.runtimeConfig
        private val codegenScope =
            arrayOf(
                *preludeScope,
                "FrameworkMetadata" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("sdk_ua_metadata::FrameworkMetadata"),
            )

        override fun section(section: ServiceConfig): Writable =
            when (section) {
                is ServiceConfig.BuilderImpl ->
                    writable {
                        rustTemplate(
                            """
                            /// Appends framework metadata to the user agent.
                            ///
                            /// This _optional_ metadata identifies a software framework or third-party library
                            /// that is being used with the client. It is rendered into the user agent string
                            /// (as `lib/{name}/{version}`) so that libraries built on top of the AWS SDK can
                            /// self-identify in the requests they make. Multiple entries may be added; each call
                            /// appends another entry rather than replacing previous ones.
                            ///
                            /// Entries are de-duplicated on `(name, version)`, rendered in first-seen order, and
                            /// the total number of unique entries included in the user agent is capped (currently
                            /// at 10); additional entries beyond the cap are dropped with a warning.
                            pub fn framework_metadata(mut self, framework_metadata: #{FrameworkMetadata}) -> Self {
                                self.push_framework_metadata(framework_metadata);
                                self
                            }
                            """,
                            *codegenScope,
                        )

                        rustTemplate(
                            """
                            /// Appends framework metadata to the user agent.
                            ///
                            /// This _optional_ metadata identifies a software framework or third-party library
                            /// that is being used with the client. It is rendered into the user agent string
                            /// (as `lib/{name}/{version}`) so that libraries built on top of the AWS SDK can
                            /// self-identify in the requests they make. Multiple entries may be added; each call
                            /// appends another entry rather than replacing previous ones.
                            pub fn push_framework_metadata(&mut self, framework_metadata: #{FrameworkMetadata}) -> &mut Self {
                                self.config.store_append(framework_metadata);
                                self
                            }
                            """,
                            *codegenScope,
                        )
                    }

                is ServiceConfig.BuilderFromConfigBag ->
                    writable {
                        rustTemplate(
                            """
                            for framework_metadata in ${section.configBag}.load::<#{FrameworkMetadata}>() {
                                ${section.builder}.push_framework_metadata(framework_metadata.clone());
                            }
                            """,
                            *codegenScope,
                        )
                    }

                is ServiceConfig.ConfigImpl ->
                    writable {
                        rustTemplate(
                            """
                            /// Returns the framework metadata that has been configured, if any.
                            ///
                            /// This _optional_ metadata identifies software frameworks or third-party libraries
                            /// being used with the client, rendered into the user agent as `lib/{name}/{version}`.
                            pub fn framework_metadata(&self) -> #{Vec}<&#{FrameworkMetadata}> {
                                self.config.load::<#{FrameworkMetadata}>().collect()
                            }
                            """,
                            *codegenScope,
                        )
                    }

                else -> emptySection
            }
    }
}
