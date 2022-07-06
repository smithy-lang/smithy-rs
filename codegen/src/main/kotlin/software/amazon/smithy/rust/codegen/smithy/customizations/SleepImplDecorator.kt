/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/* Example Generated Code */
/*
/// Service config.
///
pub struct Config {
    pub(crate) sleep_impl: Option<std::sync::Arc<dyn aws_smithy_async::rt::sleep::AsyncSleep>>,
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
    sleep_impl: Option<std::sync::Arc<dyn aws_smithy_async::rt::sleep::AsyncSleep>>,
}
impl Builder {
    /// Constructs a config builder.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the sleep_impl for the builder
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use test_smithy_test1832442648477221704::config::Config;
    /// use aws_smithy_async::rt::sleep::AsyncSleep;
    /// use aws_smithy_async::rt::sleep::Sleep;
    ///
    /// #[derive(Debug)]
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
    pub fn sleep_impl(
        mut self,
        sleep_impl: std::sync::Arc<dyn aws_smithy_async::rt::sleep::AsyncSleep>,
    ) -> Self {
        self.set_sleep_impl(Some(sleep_impl));
        self
    }

    /// Set the sleep_impl for the builder
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use test_smithy_test1832442648477221704::config::{Builder, Config};
    /// use aws_smithy_async::rt::sleep::AsyncSleep;
    /// use aws_smithy_async::rt::sleep::Sleep;
    ///
    /// #[derive(Debug)]
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
    pub fn set_sleep_impl(
        &mut self,
        sleep_impl: Option<std::sync::Arc<dyn aws_smithy_async::rt::sleep::AsyncSleep>>,
    ) -> &mut Self {
        self.sleep_impl = sleep_impl;
        self
    }
    /// Builds a [`Config`].
    pub fn build(self) -> Config {
        Config {
            sleep_impl: self.sleep_impl
        }
    }
}
 */

class SleepImplDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "AsyncSleep"
    override val order: Byte = 0

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + SleepImplProviderConfig(codegenContext)
    }
}

class SleepImplProviderConfig(coreCodegenContext: CoreCodegenContext) : ConfigCustomization() {
    private val sleepModule = smithyAsyncRtSleep(coreCodegenContext.runtimeConfig)
    private val moduleUseName = coreCodegenContext.moduleUseName()
    private val codegenScope = arrayOf(
        "AsyncSleep" to sleepModule.member("AsyncSleep"),
        "Sleep" to sleepModule.member("Sleep"),
    )

    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate(
                "pub(crate) sleep_impl: Option<std::sync::Arc<dyn #{AsyncSleep}>>,",
                *codegenScope
            )
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rustTemplate("sleep_impl: Option<std::sync::Arc<dyn #{AsyncSleep}>>,", *codegenScope)
            ServiceConfig.BuilderImpl ->
                rustTemplate(
                    """
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
                    """,
                    *codegenScope
                )
            ServiceConfig.BuilderBuild -> rustTemplate(
                """sleep_impl: self.sleep_impl,""",
                *codegenScope
            )
            else -> emptySection
        }
    }
}

// Generate path to the root module in aws_smithy_async
fun smithyAsyncRtSleep(runtimeConfig: RuntimeConfig) =
    RuntimeType("sleep", runtimeConfig.runtimeCrate("async"), "aws_smithy_async::rt")
