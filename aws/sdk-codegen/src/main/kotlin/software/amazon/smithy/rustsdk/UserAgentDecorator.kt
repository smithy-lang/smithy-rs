/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customizations.CrateVersionCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
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
        return baseCustomizations + AppNameCustomization(codegenContext.runtimeConfig)
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + UserAgentFeature(codegenContext)
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust("${section.serviceConfigBuilder}.set_app_name(${section.sdkConfig}.app_name().cloned());")
            },
        )
    }

    /**
     * Adds a static `API_METADATA` variable to the crate `config` containing the serviceId & the version of the crate for this individual service
     */
    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val runtimeConfig = codegenContext.runtimeConfig

        // We are generating an AWS SDK, the service needs to have the AWS service trait
        val serviceTrait = codegenContext.serviceShape.expectTrait<ServiceTrait>()
        val serviceId = serviceTrait.sdkId.lowercase().replace(" ", "")

        val metaModule = when (codegenContext.settings.codegenConfig.enableNewCrateOrganizationScheme) {
            true -> ClientRustModule.Meta
            else -> ClientRustModule.root
        }
        rustCrate.withModule(metaModule) {
            rustTemplate(
                """
                pub(crate) static API_METADATA: #{user_agent}::ApiMetadata =
                    #{user_agent}::ApiMetadata::new(${serviceId.dq()}, #{PKG_VERSION});
                """,
                "user_agent" to AwsRuntimeType.awsHttp(runtimeConfig).resolve("user_agent"),
                "PKG_VERSION" to CrateVersionCustomization.pkgVersion(metaModule),
            )
        }

        val configModule = when (codegenContext.settings.codegenConfig.enableNewCrateOrganizationScheme) {
            true -> ClientRustModule.Config
            else -> ClientRustModule.root
        }
        rustCrate.withModule(configModule) {
            // Re-export the app name so that it can be specified in config programmatically without an explicit dependency
            rustTemplate(
                "pub use #{AppName};",
                "AppName" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("app_name::AppName"),
            )
        }
    }

    private class UserAgentFeature(
        private val codegenContext: ClientCodegenContext,
    ) : OperationCustomization() {
        private val runtimeConfig = codegenContext.runtimeConfig

        override fun section(section: OperationSection): Writable = when (section) {
            is OperationSection.MutateRequest -> writable {
                val metaModule = when (codegenContext.settings.codegenConfig.enableNewCrateOrganizationScheme) {
                    true -> ClientRustModule.Meta
                    else -> ClientRustModule.root
                }
                rustTemplate(
                    """
                    let mut user_agent = #{ua_module}::AwsUserAgent::new_from_environment(
                        #{Env}::real(),
                        #{meta}::API_METADATA.clone(),
                    );
                    if let Some(app_name) = _config.app_name() {
                        user_agent = user_agent.with_app_name(app_name.clone());
                    }
                    ${section.request}.properties_mut().insert(user_agent);
                    """,
                    "meta" to metaModule,
                    "ua_module" to AwsRuntimeType.awsHttp(runtimeConfig).resolve("user_agent"),
                    "Env" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("os_shim_internal::Env"),
                )
            }

            else -> emptySection
        }
    }

    private class AppNameCustomization(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
        private val codegenScope = arrayOf(
            "AppName" to AwsRuntimeType.awsTypes(runtimeConfig).resolve("app_name::AppName"),
        )

        override fun section(section: ServiceConfig): Writable =
            when (section) {
                is ServiceConfig.BuilderStruct -> writable {
                    rustTemplate("app_name: Option<#{AppName}>,", *codegenScope)
                }

                is ServiceConfig.BuilderImpl -> writable {
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

                        /// Sets the name of the app that is using the client.
                        ///
                        /// This _optional_ name is used to identify the application in the user agent that
                        /// gets sent along with requests.
                        pub fn set_app_name(&mut self, app_name: Option<#{AppName}>) -> &mut Self {
                            self.app_name = app_name;
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderBuild -> writable {
                    rust("app_name: self.app_name,")
                }

                is ServiceConfig.ConfigStruct -> writable {
                    rustTemplate("app_name: Option<#{AppName}>,", *codegenScope)
                }

                is ServiceConfig.ConfigImpl -> writable {
                    rustTemplate(
                        """
                        /// Returns the name of the app that is using the client, if it was provided.
                        ///
                        /// This _optional_ name is used to identify the application in the user agent that
                        /// gets sent along with requests.
                        pub fn app_name(&self) -> Option<&#{AppName}> {
                            self.app_name.as_ref()
                        }
                        """,
                        *codegenScope,
                    )
                }

                else -> emptySection
            }
    }
}
