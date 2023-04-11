/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate

class ResiliencyConfigCustomization(codegenContext: CodegenContext) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val retryConfig = RuntimeType.smithyTypes(runtimeConfig).resolve("retry")
    private val sleepModule = RuntimeType.smithyAsync(runtimeConfig).resolve("rt::sleep")
    private val timeoutModule = RuntimeType.smithyTypes(runtimeConfig).resolve("timeout")
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        "AsyncSleep" to sleepModule.resolve("AsyncSleep"),
        "RetryConfig" to retryConfig.resolve("RetryConfig"),
        "Sleep" to sleepModule.resolve("Sleep"),
        "TimeoutConfig" to timeoutModule.resolve("TimeoutConfig"),
    )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                is ServiceConfig.ConfigStruct -> rustTemplate(
                    """
                    retry_config: Option<#{RetryConfig}>,
                    sleep_impl: Option<std::sync::Arc<dyn #{AsyncSleep}>>,
                    timeout_config: Option<#{TimeoutConfig}>,
                    """,
                    *codegenScope,
                )

                is ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Return a reference to the retry configuration contained in this config, if any.
                        pub fn retry_config(&self) -> Option<&#{RetryConfig}> {
                            self.retry_config.as_ref()
                        }

                        /// Return a cloned Arc containing the async sleep implementation from this config, if any.
                        pub fn sleep_impl(&self) -> Option<std::sync::Arc<dyn #{AsyncSleep}>> {
                            self.sleep_impl.clone()
                        }

                        /// Return a reference to the timeout configuration contained in this config, if any.
                        pub fn timeout_config(&self) -> Option<&#{TimeoutConfig}> {
                            self.timeout_config.as_ref()
                        }
                        """,
                        *codegenScope,
                    )
                }

                is ServiceConfig.BuilderStruct ->
                    rustTemplate(
                        """
                        retry_config: Option<#{RetryConfig}>,
                        sleep_impl: Option<std::sync::Arc<dyn #{AsyncSleep}>>,
                        timeout_config: Option<#{TimeoutConfig}>,
                        """,
                        *codegenScope,
                    )

                ServiceConfig.BuilderImpl ->
                    rustTemplate(
                        """
                        /// Set the retry_config for the builder
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// use $moduleUseName::config::Config;
                        /// use $moduleUseName::config::retry::RetryConfig;
                        ///
                        /// let retry_config = RetryConfig::standard().with_max_attempts(5);
                        /// let config = Config::builder().retry_config(retry_config).build();
                        /// ```
                        pub fn retry_config(mut self, retry_config: #{RetryConfig}) -> Self {
                            self.set_retry_config(Some(retry_config));
                            self
                        }

                        /// Set the retry_config for the builder
                        ///
                        /// ## Examples
                        /// ```no_run
                        /// use $moduleUseName::config::{Builder, Config};
                        /// use $moduleUseName::config::retry::RetryConfig;
                        ///
                        /// fn disable_retries(builder: &mut Builder) {
                        ///     let retry_config = RetryConfig::standard().with_max_attempts(1);
                        ///     builder.set_retry_config(Some(retry_config));
                        /// }
                        ///
                        /// let mut builder = Config::builder();
                        /// disable_retries(&mut builder);
                        /// let config = builder.build();
                        /// ```
                        pub fn set_retry_config(&mut self, retry_config: Option<#{RetryConfig}>) -> &mut Self {
                            self.retry_config = retry_config;
                            self
                        }

                        /// Set the sleep_impl for the builder
                        ///
                        /// ## Examples
                        ///
                        /// ```no_run
                        /// use $moduleUseName::config::{AsyncSleep, Sleep, Config};
                        ///
                        /// ##[derive(Debug)]
                        /// pub struct ForeverSleep;
                        ///
                        /// impl AsyncSleep for ForeverSleep {
                        ///     fn sleep(&self, duration: std::time::Duration) -> Sleep {
                        ///         Sleep::new(std::future::pending())
                        ///     }
                        /// }
                        ///
                        /// let sleep_impl = std::sync::Arc::new(ForeverSleep);
                        /// let config = Config::builder().sleep_impl(sleep_impl).build();
                        /// ```
                        pub fn sleep_impl(mut self, sleep_impl: std::sync::Arc<dyn #{AsyncSleep}>) -> Self {
                            self.set_sleep_impl(Some(sleep_impl));
                            self
                        }

                        /// Set the sleep_impl for the builder
                        ///
                        /// ## Examples
                        ///
                        /// ```no_run
                        /// use $moduleUseName::config::{AsyncSleep, Sleep, Builder, Config};
                        ///
                        /// ##[derive(Debug)]
                        /// pub struct ForeverSleep;
                        ///
                        /// impl AsyncSleep for ForeverSleep {
                        ///     fn sleep(&self, duration: std::time::Duration) -> Sleep {
                        ///         Sleep::new(std::future::pending())
                        ///     }
                        /// }
                        ///
                        /// fn set_never_ending_sleep_impl(builder: &mut Builder) {
                        ///     let sleep_impl = std::sync::Arc::new(ForeverSleep);
                        ///     builder.set_sleep_impl(Some(sleep_impl));
                        /// }
                        ///
                        /// let mut builder = Config::builder();
                        /// set_never_ending_sleep_impl(&mut builder);
                        /// let config = builder.build();
                        /// ```
                        pub fn set_sleep_impl(&mut self, sleep_impl: Option<std::sync::Arc<dyn #{AsyncSleep}>>) -> &mut Self {
                            self.sleep_impl = sleep_impl;
                            self
                        }

                        /// Set the timeout_config for the builder
                        ///
                        /// ## Examples
                        ///
                        /// ```no_run
                        /// ## use std::time::Duration;
                        /// use $moduleUseName::config::Config;
                        /// use $moduleUseName::config::timeout::TimeoutConfig;
                        ///
                        /// let timeout_config = TimeoutConfig::builder()
                        ///     .operation_attempt_timeout(Duration::from_secs(1))
                        ///     .build();
                        /// let config = Config::builder().timeout_config(timeout_config).build();
                        /// ```
                        pub fn timeout_config(mut self, timeout_config: #{TimeoutConfig}) -> Self {
                            self.set_timeout_config(Some(timeout_config));
                            self
                        }

                        /// Set the timeout_config for the builder
                        ///
                        /// ## Examples
                        ///
                        /// ```no_run
                        /// ## use std::time::Duration;
                        /// use $moduleUseName::config::{Builder, Config};
                        /// use $moduleUseName::config::timeout::TimeoutConfig;
                        ///
                        /// fn set_request_timeout(builder: &mut Builder) {
                        ///     let timeout_config = TimeoutConfig::builder()
                        ///         .operation_attempt_timeout(Duration::from_secs(1))
                        ///         .build();
                        ///     builder.set_timeout_config(Some(timeout_config));
                        /// }
                        ///
                        /// let mut builder = Config::builder();
                        /// set_request_timeout(&mut builder);
                        /// let config = builder.build();
                        /// ```
                        pub fn set_timeout_config(&mut self, timeout_config: Option<#{TimeoutConfig}>) -> &mut Self {
                            self.timeout_config = timeout_config;
                            self
                        }
                        """,
                        *codegenScope,
                    )

                ServiceConfig.BuilderBuild -> rustTemplate(
                    // We call clone on sleep_impl because the field is used by
                    // initializing the credentials_cache field later in the build
                    // method of a Config builder.
                    // We could rearrange the order of decorators so that AwsCodegenDecorator
                    // runs before RequiredCustomizations, which in turns renders
                    // CredentialsCacheDecorator before this class, but that is a bigger
                    // change than adding a call to the clone method on sleep_impl.
                    """
                    retry_config: self.retry_config,
                    sleep_impl: self.sleep_impl.clone(),
                    timeout_config: self.timeout_config,
                    """,
                    *codegenScope,
                )

                else -> emptySection
            }
        }
}

class ResiliencyReExportCustomization(private val runtimeConfig: RuntimeConfig) {
    fun extras(rustCrate: RustCrate) {
        rustCrate.withModule(RustModule.Config) {
            rustTemplate(
                """
                pub use #{sleep}::{AsyncSleep, Sleep};

                /// Retry configuration
                ///
                /// These are re-exported from `aws-smithy-types` for convenience.
                pub mod retry {
                    pub use #{types_retry}::{RetryConfig, RetryConfigBuilder, RetryMode};
                }
                /// Timeout configuration
                ///
                /// These are re-exported from `aws-smithy-types` for convenience.
                pub mod timeout {
                    pub use #{timeout}::{TimeoutConfig, TimeoutConfigBuilder};
                }
                """,
                "types_retry" to RuntimeType.smithyTypes(runtimeConfig).resolve("retry"),
                "sleep" to RuntimeType.smithyAsync(runtimeConfig).resolve("rt::sleep"),
                "timeout" to RuntimeType.smithyTypes(runtimeConfig).resolve("timeout"),
            )
        }
    }
}
