/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/* Example Generated Code */
/*
pub struct Config {
    pub(crate) retry_config: Option<smithy_types::retry::RetryConfig>,
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
    retry_config: Option<smithy_types::retry::RetryConfig>,
}
impl Builder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn retry_config(mut self, retry_config: smithy_types::retry::RetryConfig) -> Self {
        self.set_retry_config(Some(retry_config));
        self
    }
    pub fn set_retry_config(
        &mut self,
        retry_config: Option<smithy_types::retry::RetryConfig>,
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

class RetryConfigDecorator : RustCodegenDecorator {
    override val name: String = "RetryConfig"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + RetryConfigProviderConfig(codegenContext.runtimeConfig)
    }

    override fun operationCustomizations(
        codegenContext: CodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + RetryConfigConfigPlugin()
    }

    override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseRetryConfig(codegenContext.runtimeConfig)
    }
}

class RetryConfigProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val retryConfig = smithyTypesRetry(runtimeConfig)
    private val codegenScope = arrayOf("RetryConfig" to retryConfig.member("RetryConfig"))
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate("pub(crate) retry_config: Option<#{RetryConfig}>,", *codegenScope)
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rustTemplate("retry_config: Option<#{RetryConfig}>,", *codegenScope)
            ServiceConfig.BuilderImpl ->
                rustTemplate(
                    """
            pub fn retry_config(mut self, retry_config: #{RetryConfig}) -> Self {
                self.set_retry_config(Some(retry_config));
                self
            }
        
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
        }
    }
}

class RetryConfigConfigPlugin : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                // Allow the retry config to be late-inserted via another method
                rust(
                    """
                if let Some(retry_config) = &${section.config}.retry_config {
                    ${section.request}.properties_mut().insert(retry_config.clone());
                }
                """
                )
            }
            else -> emptySection
        }
    }
}

class PubUseRetryConfig(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable { rust("pub use #T::RetryConfig;", smithyTypesRetry(runtimeConfig)) }
            else -> emptySection
        }
    }
}

// Generate path to the retry module in smithy_types
fun smithyTypesRetry(runtimeConfig: RuntimeConfig) =
    RuntimeType("retry", runtimeConfig.runtimeCrate("types"), "smithy_types")
