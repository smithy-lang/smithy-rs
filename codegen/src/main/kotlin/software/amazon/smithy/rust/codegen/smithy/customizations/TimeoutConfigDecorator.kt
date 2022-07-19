/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/* Example Generated Code */
/*
pub struct Config {
    pub(crate) timeout_config: Option<aws_smithy_types::timeout::Config>,
}
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut config = f.debug_struct("Config");
        config.finish()
    }
}
impl Config {
    /// Constructs a config builder.
    pub fn builder() -> Builder {
        Builder::default()
    }
}
/// Builder for creating a `Config`.
#[derive(Default)]
pub struct Builder {
    timeout_config: Option<aws_smithy_types::timeout::Config>,
}
impl Builder {
    /// Constructs a config builder.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the timeout_config for the builder
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::time::Duration;
    /// use test_smithy_test2036416049427740159::config::Config;
    /// use aws_smithy_types::{timeout, tristate::TriState};
    ///
    /// let api_timeouts = timeout::Api::new()
    ///     .with_call_attempt_timeout(TriState::Set(Duration::from_secs(1)));
    /// let timeout_config = timeout::Config::new()
    ///     .with_api_timeouts(api_timeouts);
    /// let config = Config::builder().timeout_config(timeout_config).build();
    /// ```
    pub fn timeout_config(mut self, timeout_config: aws_smithy_types::timeout::Config) -> Self {
        self.set_timeout_config(Some(timeout_config));
        self
    }

    /// Set the timeout_config for the builder
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::time::Duration;
    /// use test_smithy_test2036416049427740159::config::{Builder, Config};
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
    pub fn set_timeout_config(
        &mut self,
        timeout_config: Option<aws_smithy_types::timeout::Config>,
    ) -> &mut Self {
        self.timeout_config = timeout_config;
        self
    }
    /// Builds a [`Config`].
    pub fn build(self) -> Config {
        Config {
            timeout_config: self.timeout_config,
        }
    }
}
#[test]
fn test_1() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Config>();
}
 */

class TimeoutConfigDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "TimeoutConfig"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + TimeoutConfigProviderConfig(codegenContext)
    }
}

class TimeoutConfigProviderConfig(coreCodegenContext: CoreCodegenContext) : ConfigCustomization() {
    private val smithyTypesCrate = coreCodegenContext.runtimeConfig.runtimeCrate("types")
    private val timeoutModule = RuntimeType("timeout", smithyTypesCrate, "aws_smithy_types")
    private val moduleUseName = coreCodegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        "TimeoutConfig" to timeoutModule.member("Config"),
    )
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate(
                "pub(crate) timeout_config: Option<#{TimeoutConfig}>,",
                *codegenScope
            )
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rustTemplate("timeout_config: Option<#{TimeoutConfig}>,", *codegenScope)
            ServiceConfig.BuilderImpl ->
                rustTemplate(
                    """
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
                    *codegenScope
                )
            ServiceConfig.BuilderBuild -> rustTemplate(
                """timeout_config: self.timeout_config,""",
                *codegenScope
            )
            else -> emptySection
        }
    }
}
