/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/* Example Generated Code */
/*
pub struct Config {
    pub(crate) retry_config: Option<aws_smithy_types::retry::RetryConfig>,
}
impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut config = f.debug_struct("Config");
        config.finish()
    }
}
impl Config {
    pub fn builder() -> Builder {
        Builder::default()
    }
}
#[derive(Default)]
pub struct Builder {
    retry_config: Option<aws_smithy_types::retry::RetryConfig>,
}
impl Builder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn retry_config(mut self, retry_config: aws_smithy_types::retry::RetryConfig) -> Self {
        self.set_retry_config(Some(retry_config));
        self
    }
    pub fn set_retry_config(
        &mut self,
        retry_config: Option<aws_smithy_types::retry::RetryConfig>,
    ) -> &mut Self {
        self.retry_config = retry_config;
        self
    }
    pub fn build(self) -> Config {
        Config {
            retry_config: self.retry_config,
        }
    }
}
#[test]
fn test_1() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Config>();
}
 */

class RetryConfigDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "RetryConfig"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + RetryConfigProviderConfig(codegenContext)
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseRetryConfig(codegenContext.runtimeConfig)
    }
}

class RetryConfigProviderConfig(coreCodegenContext: CoreCodegenContext) : ConfigCustomization() {
    private val retryConfig = smithyTypesRetry(coreCodegenContext.runtimeConfig)
    private val moduleUseName = coreCodegenContext.moduleUseName()
    private val codegenScope = arrayOf("RetryConfig" to retryConfig.member("RetryConfig"))
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate(
                "pub(crate) retry_config: Option<#{RetryConfig}>,",
                *codegenScope
            )
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rustTemplate("retry_config: Option<#{RetryConfig}>,", *codegenScope)
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
                    /// let retry_config = RetryConfig::new().with_max_attempts(5);
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
                    ///     let retry_config = RetryConfig::new().with_max_attempts(1);
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
                    """,
                    *codegenScope
                )
            ServiceConfig.BuilderBuild -> rustTemplate(
                """retry_config: self.retry_config,""",
                *codegenScope
            )
            else -> emptySection
        }
    }
}

class PubUseRetryConfig(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                rust("pub use #T::RetryConfig;", smithyTypesRetry(runtimeConfig))
            }
            else -> emptySection
        }
    }
}

// Generate path to the retry module in aws_smithy_types
fun smithyTypesRetry(runtimeConfig: RuntimeConfig) =
    RuntimeType("retry", runtimeConfig.runtimeCrate("types"), "aws_smithy_types")
