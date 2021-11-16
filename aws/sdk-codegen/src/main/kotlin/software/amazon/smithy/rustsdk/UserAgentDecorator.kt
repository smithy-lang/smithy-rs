/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.ServiceTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.util.dq
import software.amazon.smithy.rust.codegen.util.expectTrait

/**
 * Inserts a UserAgent configuration into the operation
 */
class UserAgentDecorator : RustCodegenDecorator {
    override val name: String = "UserAgent"
    override val order: Byte = 10

    override fun configCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + AppNameCustomization()
    }

    override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        // We are generating an AWS SDK, the service needs to have the AWS service trait
        val serviceTrait = codegenContext.serviceShape.expectTrait<ServiceTrait>()
        return baseCustomizations + ApiVersion(codegenContext.runtimeConfig, serviceTrait)
    }

    override fun operationCustomizations(
        codegenContext: CodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + UserAgentFeature(codegenContext.runtimeConfig)
    }
}

/**
 * Adds a static `API_METADATA` variable to the crate root containing the serviceId & the version of the crate for this individual service
 */
private class ApiVersion(private val runtimeConfig: RuntimeConfig, serviceTrait: ServiceTrait) : LibRsCustomization() {
    private val serviceId = serviceTrait.sdkId.toLowerCase().replace(" ", "")
    override fun section(section: LibRsSection): Writable = when (section) {
        // PKG_VERSION comes from CrateVersionGenerator
        is LibRsSection.Body -> writable {
            rust(
                "static API_METADATA: #1T::ApiMetadata = #1T::ApiMetadata::new(${serviceId.dq()}, PKG_VERSION);",
                runtimeConfig.userAgentModule()
            )
        }
        else -> emptySection
    }
}

private fun RuntimeConfig.userAgentModule() = awsHttp().asType().copy(name = "user_agent")
private fun RuntimeConfig.env(): RuntimeType = RuntimeType("Env", awsTypes(), "aws_types::os_shim_internal")

private class UserAgentFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.MutateRequest -> writable {
            rustTemplate(
                """
                ${section.request}.properties_mut().insert(
                    #{ua_module}::AwsUserAgent::new_from_environment(
                        #{Env}::real(),
                        crate::API_METADATA.clone(),
                        Vec::new(),
                        Vec::new(),
                        Vec::new(),
                        _config.app_name().cloned(),
                    )
                );
                """,
                "ua_module" to runtimeConfig.userAgentModule(),
                "Env" to runtimeConfig.env(),
            )
        }
        else -> emptySection
    }
}

private class AppNameCustomization() : ConfigCustomization() {
    override fun section(section: ServiceConfig): Writable =
        when (section) {
            is ServiceConfig.BuilderStruct -> writable {
                rust("app_name: Option<std::borrow::Cow<'static, str>>,")
            }
            is ServiceConfig.BuilderImpl -> writable {
                rust(
                    """
                    /// Sets the name of the app that is using the client.
                    ///
                    /// This _optional_ name is used to identify the application in the user agent that
                    /// gets sent along with requests.
                    ///
                    /// The name may only have alphanumeric characters and any of these characters:
                    /// ```text
                    /// !##${'$'}%&'*+-.^_`|~
                    /// ```
                    /// Spaces are not allowed. If unsupported characters are given, this will lead to a panic.
                    ///
                    /// App names are recommended to be no more than 50 characters.
                    pub fn app_name(mut self, app_name: impl Into<std::borrow::Cow<'static, str>>) -> Self {
                        self.set_app_name(Some(app_name.into()));
                        self
                    }

                    /// Sets the name of the app that is using the client.
                    ///
                    /// This _optional_ name is used to identify the application in the user agent that
                    /// gets sent along with requests.
                    ///
                    /// The name may only have alphanumeric characters and any of these characters:
                    /// ```text
                    /// !##${'$'}%&'*+-.^_`|~
                    /// ```
                    /// Spaces are not allowed. If unsupported characters are given, this will lead to a panic.
                    ///
                    /// App names are recommended to be no more than 50 characters.
                    pub fn set_app_name(&mut self, app_name: Option<std::borrow::Cow<'static, str>>) -> &mut Self {
                        self.app_name = app_name;
                        self
                    }
                    """
                )
            }
            is ServiceConfig.BuilderBuild -> writable {
                rust("app_name: self.app_name,")
            }
            is ServiceConfig.ConfigStruct -> writable {
                rust("app_name: Option<std::borrow::Cow<'static, str>>,")
            }
            is ServiceConfig.ConfigImpl -> writable {
                rust(
                    """
                    /// Returns the name of the app that is using the client, if it was provided.
                    ///
                    /// This _optional_ name is used to identify the application in the user agent that
                    /// gets sent along with requests.
                    pub fn app_name(&self) -> Option<&std::borrow::Cow<'static, str>> {
                        self.app_name.as_ref()
                    }
                    """
                )
            }
            else -> emptySection
        }
}
