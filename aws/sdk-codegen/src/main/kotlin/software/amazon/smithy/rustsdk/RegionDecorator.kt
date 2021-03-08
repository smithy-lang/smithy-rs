/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Local
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
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
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<ConfigCustomization>
    ): List<ConfigCustomization> {
        return baseCustomizations + RegionProviderConfig(protocolConfig.runtimeConfig)
    }

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + RegionConfigPlugin()
    }

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + PubUseRegion(protocolConfig.runtimeConfig)
    }
}

class RegionProviderConfig(runtimeConfig: RuntimeConfig) : ConfigCustomization() {
    private val region = region(runtimeConfig)
    override fun section(section: ServiceConfig) = writable {
        when (section) {
            is ServiceConfig.ConfigStruct -> rust("pub(crate) region: Option<#T::Region>,", region)
            is ServiceConfig.ConfigImpl -> emptySection
            is ServiceConfig.BuilderStruct ->
                rust("region: Option<#T::Region>,", region)
            ServiceConfig.BuilderImpl ->
                rust(
                    """
            pub fn region(mut self, region_provider: impl #1T::ProvideRegion) -> Self {
                self.region = region_provider.region();
                self
            }
            """,
                    region
                )
            ServiceConfig.BuilderBuild -> rust(
                """region: {
                    use #1T::ProvideRegion;
                    self.region.or_else(||#1T::default_provider().region())
                },""",
                region
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
                    ${section.request}.config_mut().insert(region.clone());
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
        }
    }
}

fun region(runtimeConfig: RuntimeConfig) =
    RuntimeType("region", awsTypes(runtimeConfig), "aws_types")

fun awsTypes(runtimeConfig: RuntimeConfig) = CargoDependency("aws-types", Local(runtimeConfig.relativePath))
