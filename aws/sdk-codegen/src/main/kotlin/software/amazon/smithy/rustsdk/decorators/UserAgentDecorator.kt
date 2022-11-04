/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.decorators

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rustsdk.AwsCustomization
import software.amazon.smithy.rustsdk.AwsSection
import software.amazon.smithy.rustsdk.awsHttp
import software.amazon.smithy.rustsdk.awsTypes

/**
 * Inserts a UserAgent configuration into the operation
 */
class UserAgentDecorator : AwsCodegenDecorator {
    override val order: Byte = 10

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + AppNameCustomization(codegenContext.runtimeConfig)
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        // We are generating an AWS SDK, the service needs to have the AWS service trait
        val serviceTrait = codegenContext.serviceShape.expectTrait<ServiceTrait>()
        return baseCustomizations + ApiVersionAndPubUse(codegenContext.runtimeConfig, serviceTrait)
    }

    override fun awsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<AwsCustomization>,
    ): List<AwsCustomization> {
        return baseCustomizations + AppNameFromSdkConfig()
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + UserAgentFeature(codegenContext.runtimeConfig)
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

/**
 * Adds a static `API_METADATA` variable to the crate root containing the serviceId & the version of the crate for this individual service
 */
private class ApiVersionAndPubUse(private val runtimeConfig: RuntimeConfig, serviceTrait: ServiceTrait) :
    LibRsCustomization() {
    private val serviceId = serviceTrait.sdkId.lowercase().replace(" ", "")
    override fun section(section: LibRsSection): Writable = when (section) {
        is LibRsSection.Body -> writable {
            // PKG_VERSION comes from CrateVersionGenerator
            rustTemplate(
                "static API_METADATA: #{ApiMetadata} = #{ApiMetadata}::new(${serviceId.dq()}, PKG_VERSION);",
                "ApiMetadata" to runtimeConfig.awsHttp().member("user_agent::ApiMetadata"),
            )

            // Re-export the app name so that it can be specified in config programmatically without an explicit dependency
            rust("pub use #T;", runtimeConfig.awsTypes().member("app_name::AppName"))
        }
        else -> emptySection
    }
}

private class UserAgentFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            rustTemplate(
                """
                let mut user_agent = #{AwsUserAgent}::new_from_environment(
                    #{Env}::real(),
                    crate::API_METADATA.clone(),
                );
                if let Some(app_name) = _config.app_name() {
                    user_agent = user_agent.with_app_name(app_name.clone());
                }
                ${section.request}.properties_mut().insert(user_agent);
                """,
                "AwsUserAgent" to runtimeConfig.awsHttp().member("user_agent::AwsUserAgent"),
                "Env" to runtimeConfig.awsTypes().member("os_shim_internal::Env"),
            )
        }
        else -> emptySection
    }
}

private class AppNameCustomization(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val codegenScope = arrayOf("AppName" to runtimeConfig.awsTypes().member("app_name::AppName"))

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

class AppNameFromSdkConfig : AwsCustomization() {
    override fun section(section: AwsSection): Writable = writable {
        when (section) {
            is AwsSection.FromSdkConfigForBuilder -> rust("builder.set_app_name(input.app_name().cloned());")
        }
    }
}
