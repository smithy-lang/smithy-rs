/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.rulesengine.language.syntax.parameters.Builtins
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.generators.CustomRuntimeFunction
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.dq

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

class RegionDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "Region"
    override val order: Byte = 0

    override fun transformModel(service: ServiceShape, model: Model): Model {
        return super.transformModel(service, model)
    }

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations + RegionProviderConfig(codegenContext)
    }

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + RegionConfigPlugin()
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseRegion(codegenContext.runtimeConfig)
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        return listOf(
            object : EndpointCustomization {
                override fun builtInDefaultValue(parameter: Parameter, configRef: String): Writable? {
                    return when (parameter) {
                        Builtins.REGION -> writable { rust("$configRef.region.as_ref().map(|r|r.as_ref().to_owned())") }
                        else -> null
                    }
                }

                override fun setBuiltInOnConfig(name: String, value: Node, configBuilderRef: String): Writable? {
                    if (name != Builtins.REGION.builtIn.get()) {
                        println("not handling: $name")
                        return null
                    }
                    return writable {
                        rustTemplate(
                            "let $configBuilderRef = $configBuilderRef.region(#{Region}::new(${value.expectStringNode().value.dq()}));",
                            "Region" to region(codegenContext.runtimeConfig).resolve("Region"),
                        )
                    }
                }

                override fun customRuntimeFunctions(codegenContext: ClientCodegenContext): List<CustomRuntimeFunction> =
                    listOf()
            },
        )
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class RegionProviderConfig(codegenContext: CodegenContext) : ConfigCustomization() {
    private val region = region(codegenContext.runtimeConfig)
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope = arrayOf("Region" to region.resolve("Region"))
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            ServiceConfig.ConfigStruct -> rustTemplate("pub(crate) region: Option<#{Region}>,", *codegenScope)
            ServiceConfig.ConfigImpl -> rustTemplate(
                """
                /// Returns the AWS region, if it was provided.
                pub fn region(&self) -> Option<&#{Region}> {
                    self.region.as_ref()
                }
                """,
                *codegenScope,
            )

            ServiceConfig.BuilderStruct ->
                rustTemplate("region: Option<#{Region}>,", *codegenScope)

            ServiceConfig.BuilderImpl ->
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
                    pub fn region(mut self, region: impl Into<Option<#{Region}>>) -> Self {
                        self.region = region.into();
                        self
                    }
                    """,
                    *codegenScope,
                )

            ServiceConfig.BuilderBuild -> rustTemplate(
                """region: self.region,""",
                *codegenScope,
            )

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

class PubUseRegion(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable {
                rust(
                    "pub use #T::Region;",
                    region(runtimeConfig),
                )
            }

            else -> emptySection
        }
    }
}

fun region(runtimeConfig: RuntimeConfig) = AwsRuntimeType.awsTypes(runtimeConfig).resolve("region")
