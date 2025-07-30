/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocSection
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.writeCustomizations

sealed class ServiceConfigSection(name: String) : AdHocSection(name) {
    data class MergeFromSharedConfig(val sdkConfig: String) : SdkConfigSection("MergeFromSharedConfig")

    data class LoadFromServiceSpecificEnv(val sdkConfig: String, val serviceConfigBuilder: String) :
        SdkConfigSection("LoadFromServiceSpecificEnv")
}

class ServiceConfigDecorator : ClientCodegenDecorator {
    override val name: String = "ServiceConfigGenerator"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + ServiceConfigCustomization(codegenContext)

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rustTemplate(
                    """
                    ${section.serviceConfigBuilder}.sdk_config = #{Some}(input.clone());
                    """,
                    *preludeScope,
                )
            },
        )
}

private class ServiceConfigCustomization(private val codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val rc = codegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Env" to AwsRuntimeType.awsTypes(rc).resolve("os_shim_internal::Env"),
            "EnvConfigLoader" to
                AwsRuntimeType.awsRuntime(rc)
                    .resolve("env_config::EnvConfigLoader"),
            "SdkConfig" to AwsRuntimeType.awsTypes(rc).resolve("sdk_config::SdkConfig"),
            "SharedServiceConfigLoader" to AwsRuntimeType.awsTypes(rc).resolve("service_config::SharedServiceConfigLoader"),
            "Storable" to RuntimeType.smithyTypes(rc).resolve("config_bag::Storable"),
            "StoreReplace" to RuntimeType.smithyTypes(rc).resolve("config_bag::StoreReplace"),
        )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.BuilderStruct ->
                writable {
                    rustTemplate(
                        """
                        sdk_config: #{Option}<#{SdkConfig}>,
                        default_env_config_loader: #{Option}<#{SharedServiceConfigLoader}>,
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderImplDefaultFieldInit ->
                writable {
                    rustTemplate(
                        """
                        sdk_config: #{Default}::default(),
                        default_env_config_loader: #{Default}::default(),
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderBuildMergeFromSharedConfig ->
                // The order is important, need to handle service specific environment first and then
                // handle copying values from SdkConfig if needed.
                writable {
                    rust(
                        """
                        self.load_from_service_specific_env();
                        self.merge_from_shared_config();
                        """,
                    )
                }

            is ServiceConfig.Extras -> {
                writable {
                    rustTemplate(
                        """
                        fn load_from_service_specific_env(&mut self) {
                            let env_config_loader = match (
                                self.sdk_config
                                    .as_ref()
                                    .and_then(|cfg| cfg.service_config_shared()),
                                self.default_env_config_loader.as_ref().cloned(),
                            ) {
                                (#{Some}(from_sdk_config), _) => from_sdk_config,
                                (#{None}, #{Some}(from_service_default)) => from_service_default,
                                (#{None}, #{None}) => #{EnvConfigLoader}::default().into_shared(),
                            };
                            #{load:W}
                        }
                        """,
                        *codegenScope,
                        "load" to
                            writable {
                                writeCustomizations(
                                    codegenContext.rootDecorator.extraSections(codegenContext),
                                    ServiceConfigSection.LoadFromServiceSpecificEnv(
                                        sdkConfig = "input",
                                        serviceConfigBuilder = "builder",
                                    ),
                                )
                            },
                    )
                    rustBlock("fn merge_from_shared_config(&mut self)") {
                        rustTemplate(
                            """
                            // call `.take()` so we can call mutable methods on self within the block.
                            // The `sdk_config` field won't be used once this method exits.
                            if let #{Some}(sdk_config) = self.sdk_config.take() {
                                #{merge:W}
                            }
                            """,
                            *codegenScope,
                            "merge" to
                                writable {
                                    writeCustomizations(
                                        codegenContext.rootDecorator.extraSections(codegenContext),
                                        ServiceConfigSection.MergeFromSharedConfig(
                                            sdkConfig = "sdk_config",
                                        ),
                                    )
                                },
                        )
                    }
                    rustTemplate(
                        """
                        // Returns true if the config value is never set in the service config builder.
                        // In other words, the setter has never been invoked on the config builder.
                        fn field_never_set<T>(&self) -> bool
                        where
                            T: #{Storable}<Storer = #{StoreReplace}<T>>,
                        {
                            self.config.load::<T>().is_none() && !self.config.is_explicitly_unset::<T>()
                        }

                        // Returns true if it is programmatically set in shared config.
                        fn explicitly_set_in_shared_config(&self, origin_key: &'static str) -> bool {
                            self.sdk_config
                                .as_ref()
                                .map(|c| c.get_origin(origin_key).is_client_config())
                                .unwrap_or_default()
                        }
                        """,
                        *codegenScope,
                    )

                    Attribute(Attribute.cfg(Attribute.any(Attribute.feature(TestUtilFeature.name), writable("test"))))
                        .render(this)
                    Attribute.DocHidden.render(this)
                    rustBlock("pub fn with_env<'a>(mut self, vars: &[(&'a str, &'a str)]) -> Self") {
                        rustTemplate(
                            """
                            self.default_env_config_loader = #{Some}(
                                #{EnvConfigLoader}::builder()
                                    .env(#{Env}::from_slice(vars))
                                    .build()
                                    .into_shared(),
                            );
                            self
                            """,
                            *codegenScope,
                        )
                    }
                }
            }

            is ServiceConfig.ConfigStructAdditionalDocs -> {
                writable {
                    docs(
                        """Service configuration allows for customization of endpoints, region, credentials providers,
                        and retry configuration. Generally, it is constructed automatically for you from a shared
                        configuration loaded by the `aws-config` crate. For example:

                        ```ignore
                        // Load a shared config from the environment
                        let shared_config = aws_config::from_env().load().await;
                        // The client constructor automatically converts the shared config into the service config
                        let client = Client::new(&shared_config);
                        ```

                        The service config can also be constructed manually using its builder.
                        """,
                    )
                }
            }

            else -> emptySection
        }
    }
}
