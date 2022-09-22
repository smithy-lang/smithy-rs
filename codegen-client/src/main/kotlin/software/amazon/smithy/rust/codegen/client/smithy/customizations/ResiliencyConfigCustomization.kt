/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection

class ResiliencyConfigCustomization(coreCodegenContext: CoreCodegenContext) : ConfigCustomization() {
    private val retryConfig = smithyTypesRetry(coreCodegenContext.runtimeConfig)
    private val sleepModule = smithyAsyncRtSleep(coreCodegenContext.runtimeConfig)
    private val timeoutModule = smithyTypesTimeout(coreCodegenContext.runtimeConfig)
    private val moduleUseName = coreCodegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        "AsyncSleep" to sleepModule.member("AsyncSleep"),
        "RetryConfig" to retryConfig.member("RetryConfig"),
        "Sleep" to sleepModule.member("Sleep"),
        "TimeoutConfig" to timeoutModule.member("Config"),
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
                    /// use #{RetryConfig};
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
                    /// use #{RetryConfig};
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
                    /// use $moduleUseName::config::Config;
                    /// use #{AsyncSleep};
                    /// use #{Sleep};
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
                    /// use $moduleUseName::config::{Builder, Config};
                    /// use #{AsyncSleep};
                    /// use #{Sleep};
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
                    /// use aws_smithy_types::{timeout, tristate::TriState};
                    ///
                    /// let api_timeouts = timeout::Api::new()
                    ///     .with_call_attempt_timeout(TriState::Set(Duration::from_secs(1)));
                    /// let timeout_config = timeout::Config::new()
                    ///     .with_api_timeouts(api_timeouts);
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
                    /// use aws_smithy_types::{timeout, tristate::TriState};
                    ///
                    /// fn set_request_timeout(builder: &mut Builder) {
                    ///     let api_timeouts = timeout::Api::new()
                    ///         .with_call_attempt_timeout(TriState::Set(Duration::from_secs(1)));
                    ///     let timeout_config = timeout::Config::new()
                    ///         .with_api_timeouts(api_timeouts);
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
                    """
                retry_config: self.retry_config,
                sleep_impl: self.sleep_impl,
                timeout_config: self.timeout_config,
                """,
                    *codegenScope,
                )

                else -> emptySection
            }
        }
}

class ResiliencyReExportCustomization(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                rustTemplate(
                    """
                    pub use #{retry}::RetryConfig;
                    pub use #{sleep}::AsyncSleep;
                    pub use #{timeout}::Config as TimeoutConfig;
                    """,
                    "retry" to smithyTypesRetry(runtimeConfig),
                    "sleep" to smithyAsyncRtSleep(runtimeConfig),
                    "timeout" to smithyTypesTimeout(runtimeConfig),
                )
            }
            else -> emptySection
        }
    }
}

// Generate path to the retry module in aws_smithy_types
private fun smithyTypesRetry(runtimeConfig: RuntimeConfig) =
    RuntimeType("retry", runtimeConfig.runtimeCrate("types"), "aws_smithy_types")

// Generate path to the root module in aws_smithy_async
private fun smithyAsyncRtSleep(runtimeConfig: RuntimeConfig) =
    RuntimeType("sleep", runtimeConfig.runtimeCrate("async"), "aws_smithy_async::rt")

// Generate path to the timeout module in aws_smithy_types
private fun smithyTypesTimeout(runtimeConfig: RuntimeConfig) =
    RuntimeType("timeout", runtimeConfig.runtimeCrate("types"), "aws_smithy_types")
