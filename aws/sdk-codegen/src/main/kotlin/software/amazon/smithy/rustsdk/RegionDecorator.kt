/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.extendIf
import software.amazon.smithy.rust.codegen.core.util.thenSingletonListOf

/* Example Generated Code */
/*
pub struct Config {
    pub(crate) region: Option<aws_types::region::Region>,
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
    region: Option<aws_types::region::Region>,
}

impl Builder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn region(mut self, region: impl Into<Option<aws_types::region::Region>>) -> Self {
        self.region = region.into();
        self
    }

    pub fn build(self) -> Config {
        Config {
            region: self.region,
        }
    }
}

#[test]
fn test_1() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Config>();
}
 */

class RegionDecorator : ClientCodegenDecorator {
    override val name: String = "Region"
    override val order: Byte = 0

    private fun usesRegion(codegenContext: ClientCodegenContext) = codegenContext.getBuiltIn(Builtins.REGION) != null

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.extendIf(usesRegion(codegenContext)) {
            RegionProviderConfig(codegenContext)
        }
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations.extendIf(usesRegion(codegenContext)) { RegionConfigPlugin() }
    }

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> {
        return usesRegion(codegenContext).thenSingletonListOf {
            adhocCustomization<SdkConfigSection.CopySdkConfigToClientConfig> { section ->
                rust(
                    """
                    ${section.serviceConfigBuilder} =
                         ${section.serviceConfigBuilder}.region(${section.sdkConfig}.region().cloned());
                    """,
                )
            }
        }
    }

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        if (usesRegion(codegenContext)) {
            rustCrate.withModule(ClientRustModule.config) {
                rust("pub use #T::Region;", region(codegenContext.runtimeConfig))
            }
        }
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        if (!usesRegion(codegenContext)) {
            return listOf()
        }
        return listOf(
            object : EndpointCustomization {
                override fun loadBuiltInFromServiceConfig(parameter: Parameter, configRef: String): Writable? {
                    return when (parameter.builtIn) {
                        Builtins.REGION.builtIn -> writable {
                            if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
                                rustTemplate(
                                    "$configRef.load::<#{Region}>().map(|r|r.as_ref().to_owned())",
                                    "Region" to region(codegenContext.runtimeConfig).resolve("Region"),
                                )
                            } else {
                                rust("$configRef.region.as_ref().map(|r|r.as_ref().to_owned())")
                            }
                        }
                        else -> null
                    }
                }

                override fun setBuiltInOnServiceConfig(name: String, value: Node, configBuilderRef: String): Writable? {
                    if (name != Builtins.REGION.builtIn.get()) {
                        return null
                    }
                    return writable {
                        rustTemplate(
                            "let $configBuilderRef = $configBuilderRef.region(#{Region}::new(${value.expectStringNode().value.dq()}));",
                            "Region" to region(codegenContext.runtimeConfig).resolve("Region"),
                        )
                    }
                }
            },
        )
    }
}

class RegionProviderConfig(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val region = region(codegenContext.runtimeConfig)
    private val moduleUseName = codegenContext.moduleUseName()
    private val runtimeMode = codegenContext.smithyRuntimeMode
    private val codegenScope = arrayOf(
        *preludeScope,
        "Region" to region.resolve("Region"),
    )
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.ConfigStruct -> {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("pub(crate) region: #{Option}<#{Region}>,", *codegenScope)
                }
            }
            ServiceConfig.ConfigImpl -> {
                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Returns the AWS region, if it was provided.
                        pub fn region(&self) -> #{Option}<&#{Region}> {
                            self.config.load::<#{Region}>()
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        /// Returns the AWS region, if it was provided.
                        pub fn region(&self) -> #{Option}<&#{Region}> {
                            self.region.as_ref()
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            ServiceConfig.BuilderStruct -> {
                if (runtimeMode.generateMiddleware) {
                    rustTemplate("pub(crate) region: #{Option}<#{Region}>,", *codegenScope)
                }
            }

            ServiceConfig.BuilderImpl -> {
                rustTemplate(
                    """
                    /// Sets the AWS region to use when making requests.
                    ///
                    /// ## Examples
                    /// ```no_run
                    /// use aws_types::region::Region;
                    /// use $moduleUseName::config::{Builder, Config};
                    ///
                    /// let config = $moduleUseName::Config::builder()
                    ///     .region(Region::new("us-east-1"))
                    ///     .build();
                    /// ```
                    pub fn region(mut self, region: impl #{Into}<#{Option}<#{Region}>>) -> Self {
                        self.set_region(region.into());
                        self
                    }
                    """,
                    *codegenScope,
                )

                if (runtimeMode.generateOrchestrator) {
                    rustTemplate(
                        """
                        /// Sets the AWS region to use when making requests.
                        pub fn set_region(&mut self, region: #{Option}<#{Region}>) -> &mut Self {
                            self.config.store_or_unset(region);
                            self
                        }
                        """,
                        *codegenScope,
                    )
                } else {
                    rustTemplate(
                        """
                        /// Sets the AWS region to use when making requests.
                        pub fn set_region(&mut self, region: #{Option}<#{Region}>) -> &mut Self {
                            self.region = region;
                            self
                        }
                        """,
                        *codegenScope,
                    )
                }
            }

            ServiceConfig.BuilderBuild -> {
                if (runtimeMode.generateMiddleware) {
                    rust("region: self.region,")
                }
            }

            else -> emptySection
        }
    }
}

class RegionConfigPlugin : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        return when (section) {
            is OperationSection.MutateRequest -> writable {
                // Allow the region to be late-inserted via another method
                rust(
                    """
                    if let Some(region) = &${section.config}.region {
                        ${section.request}.properties_mut().insert(region.clone());
                    }
                    """,
                )
            }

            else -> emptySection
        }
    }
}

fun region(runtimeConfig: RuntimeConfig) = AwsRuntimeType.awsTypes(runtimeConfig).resolve("region")
