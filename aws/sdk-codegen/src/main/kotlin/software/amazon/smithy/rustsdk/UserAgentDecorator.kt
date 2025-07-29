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
        return baseCustomizations + AppNameCustomization(codegenContext)
    }

    override fun serviceRuntimePluginCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ServiceRuntimePluginCustomization>,
    ): List<ServiceRuntimePluginCustomization> = baseCustomizations + AddApiMetadataIntoConfigBag(codegenContext)

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<ServiceConfigSection.MergeFromSharedConfig> { section ->
                rustTemplate(
                    """
                    if self.field_never_set::<#{AppName}>() {
                        self.set_app_name(${section.sdkConfig}.app_name().cloned());
                    }
                    """,
                    "AppName" to AwsRuntimeType.awsTypes(codegenContext.runtimeConfig).resolve("app_name::AppName"),
                )
            },
        )

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
                        rust(
                            "${section.layer}.store_put(#T.clone());",
                            ClientRustModule.Meta.toType().resolve("API_METADATA"),
                        )
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
}
