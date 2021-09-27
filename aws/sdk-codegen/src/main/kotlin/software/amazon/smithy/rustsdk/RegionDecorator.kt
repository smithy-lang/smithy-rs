/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSettings
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.config.ServiceConfig

/* Example Generated Code */
/*
pub struct Config {
    pub region: Option<::aws_types::region::Region>,
}
#[derive(Default)]
pub struct Builder {
    region: Option<::aws_types::region::Region>,
}
impl Builder {
    pub fn region(mut self, region_provider: impl ::aws_types::region::ProvideRegion) -> Self {
        self.region = region_provider.region();
        self
    }

    pub fn build(self) -> Config {
        Config {
            region: {
                use ::aws_types::region::ProvideRegion;
                self.region
                    .or_else(|| ::aws_types::region::default_provider().region())
            },
    }
}
 */

class RegionDecorator : RustCodegenDecorator {
    override val name: String = "Region"
    override val order: Byte = 0

    override fun configCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + RegionProviderConfig(protocolConfig.runtimeConfig)
    }

    override fun operationCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + RegionConfigPlugin()
    }

    override fun libRsCustomizations(
        rustSettings: RustSettings,
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseRegion(protocolConfig.runtimeConfig)
    }
}

class RegionProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val region = region(runtimeConfig)
    private val codegenScope = arrayOf("Region" to region.member("Region"))
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rustTemplate("pub(crate) region: Option<#{Region}>,", *codegenScope)
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rustTemplate("region: Option<#{Region}>,", *codegenScope)
            ServiceConfig.BuilderImpl ->
                rustTemplate(
                    """
            pub fn region(mut self, region: impl Into<Option<#{Region}>>) -> Self {
                self.region = region.into();
                self
            }
            """,
                    *codegenScope
                )
            ServiceConfig.BuilderBuild -> rustTemplate(
                """region: self.region,""",
                *codegenScope
            )
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
                """
                )
            }
            else -> emptySection
        }
    }
}

class PubUseRegion(private val runtimeConfig: RuntimeConfig) : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable {
        return when (section) {
            is LibRsSection.Body -> writable { rust("pub use #T::Region;", region(runtimeConfig)) }
            else -> emptySection
        }
    }
}

fun region(runtimeConfig: RuntimeConfig) =
    RuntimeType("region", awsTypes(runtimeConfig), "aws_types")

fun awsTypes(runtimeConfig: RuntimeConfig) = runtimeConfig.awsRuntimeDependency("aws-types")
