/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.customize.TestUtilFeature
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.docs
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization

private val testUtilOnly =
    Attribute(
        Attribute.cfg(
            Attribute.any(
                Attribute.feature(TestUtilFeature.name),
                writable("test"),
            ),
        ),
    )

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
                    if let #{Some}(service_config_loader) = ${section.sdkConfig}.service_config_shared() {
                         ${section.serviceConfigBuilder}.set_service_config_loader(service_config_loader);
                     }
                    """,
                    *preludeScope,
                )
            },
        )

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val rc = codegenContext.runtimeConfig
        val codegenScope =
            arrayOf(
                *preludeScope,
                "Env" to AwsRuntimeType.awsTypes(rc).resolve("os_shim_internal::Env"),
                "EnvConfigValue" to AwsRuntimeType.awsRuntime(rc).resolve("env_config::EnvConfigValue"),
                "LoadServiceConfig" to AwsRuntimeType.awsTypes(rc).resolve("service_config::LoadServiceConfig"),
                "HashMap" to RuntimeType.HashMap,
                "Origin" to AwsRuntimeType.awsTypes(rc).resolve("origin::Origin"),
                "ServiceConfigKey" to AwsRuntimeType.awsTypes(rc).resolve("service_config::ServiceConfigKey"),
                "SharedServiceConfigLoader" to
                    AwsRuntimeType.awsTypes(rc)
                        .resolve("service_config::SharedServiceConfigLoader"),
            )
        rustCrate.withModule(ClientRustModule.config) {
            // Attribute.AllowDeadCode.render(this)
            rustTemplate(
                """
                ##[derive(Debug, Default)]
                struct ServiceEnv {
                    env: #{Env},
                }

                impl #{LoadServiceConfig} for ServiceEnv {
                    fn load_config(&self, key: #{ServiceConfigKey}<'_>) -> #{Option}<#{String}> {
                        let (value, _) = #{EnvConfigValue}::new()
                            .env(key.env())
                            .profile(key.profile())
                            .service_id(key.service_id())
                            .load(&self.env, #{None})?;

                        #{Some}(value.to_string())
                    }
                }
                """,
                *codegenScope,
            )

            testUtilOnly.render(this)
            rustTemplate(
                """
                impl ServiceEnv {
                    pub fn for_tests<'a>(vars: &[(&'a str, &'a str)]) -> Self {
                        Self {
                            env: #{Env}::from_slice(vars),
                        }
                    }
                }
                """,
                *codegenScope,
            )

            rustTemplate(
                """
                fn use_service_env<P>(
                    field_unset: P,
                    field_name: &'static str,
                    config_origins: &#{HashMap}<&'static str, #{Origin}>,
                ) -> bool
                where
                    P: Fn() -> bool,
                {
                    field_unset() || (
                        match config_origins.get(field_name) {
                            #{Some}(origin) => !origin.is_client_config(),
                            #{None} => true,
                        }
                    )
                }
                """,
                *codegenScope,
            )
        }
    }
}

private class ServiceConfigCustomization(clientCodegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val rc = clientCodegenContext.runtimeConfig
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Env" to AwsRuntimeType.awsTypes(rc).resolve("os_shim_internal::Env"),
            "HashMap" to RuntimeType.HashMap,
            "Origin" to AwsRuntimeType.awsTypes(rc).resolve("origin::Origin"),
            "SharedServiceConfigLoader" to
                AwsRuntimeType.awsTypes(rc)
                    .resolve("service_config::SharedServiceConfigLoader"),
        )

    override fun section(section: ServiceConfig): Writable {
        return when (section) {
            is ServiceConfig.BuilderStruct ->
                writable {
                    rustTemplate(
                        """
                        service_config_loader: #{SharedServiceConfigLoader},
                        config_origins: #{HashMap}<&'static str, #{Origin}>,
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderImpl ->
                writable {
                    rustTemplate(
                        """
                        fn set_service_config_loader(
                            &mut self,
                            service_config_loader: #{SharedServiceConfigLoader}
                        ) -> &mut Self {
                            self.service_config_loader = service_config_loader;
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.BuilderImplDefaultFieldInit ->
                writable {
                    rustTemplate(
                        """
                        service_config_loader: #{SharedServiceConfigLoader}::new(ServiceEnv::default()),
                        config_origins: #{HashMap}::new(),
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.DefaultForTests ->
                writable {
                    rustTemplate(
                        """
                        self.service_config_loader = #{SharedServiceConfigLoader}::new(ServiceEnv::for_tests(&[]));
                        """,
                        *codegenScope,
                    )
                }

            is ServiceConfig.Extras ->
                writable {
                    testUtilOnly.render(this)
                    Attribute.DocHidden.render(this)
                    rustBlock("pub fn with_env<'a>(mut self, vars: &[(&'a str, &'a str)]) -> Self") {
                        rustTemplate(
                            """
                            self.service_config_loader = #{SharedServiceConfigLoader}::new(ServiceEnv::for_tests(vars));
                            self
                            """,
                            *codegenScope,
                        )
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
