/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.auth.SigV4Trait
import software.amazon.smithy.model.knowledge.ServiceIndex
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.rulesengine.aws.language.functions.AwsBuiltIns
import software.amazon.smithy.rulesengine.language.syntax.parameters.Parameter
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.configReexport
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.customize.AdHocCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.adhocCustomization
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.extendIf

// Example Generated Code
// ----------------------
//
// pub struct Config {
//     pub(crate) region: Option<aws_types::region::Region>,
// }
//
// impl std::fmt::Debug for Config {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         let mut config = f.debug_struct("Config");
//         config.finish()
//     }
// }
//
// impl Config {
//     pub fn builder() -> Builder {
//         Builder::default()
//     }
// }
//
// #[derive(Default)]
// pub struct Builder {
//     region: Option<aws_types::region::Region>,
// }
//
// impl Builder {
//     pub fn new() -> Self {
//         Self::default()
//     }
//
//     pub fn region(mut self, region: impl Into<Option<aws_types::region::Region>>) -> Self {
//         self.region = region.into();
//         self
//     }
//
//     pub fn build(self) -> Config {
//         Config {
//             region: self.region,
//         }
//     }
// }
//
// #[test]
// fn test_1() {
//     fn assert_send_sync<T: Send + Sync>() {}
//     assert_send_sync::<Config>();
// }
//

class RegionDecorator : ClientCodegenDecorator {
    override val name: String = "Region"
    override val order: Byte = 0
    private val envKey = "AWS_REGION".dq()
    private val profileKey = "region".dq()

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> {
        return baseCustomizations.extendIf(usesRegion(codegenContext)) {
            RegionProviderConfig(codegenContext)
        }
    }

    override fun endpointCustomizations(codegenContext: ClientCodegenContext): List<EndpointCustomization> {
        if (!usesRegion(codegenContext)) {
            return listOf()
        }
        return listOf(
            object : EndpointCustomization {
                override fun loadBuiltInFromServiceConfig(
                    parameter: Parameter,
                    configRef: String,
                ): Writable? {
                    return when (parameter.builtIn) {
                        AwsBuiltIns.REGION.builtIn ->
                            writable {
                                rustTemplate(
                                    "$configRef.load::<#{Region}>().map(|r|r.as_ref().to_owned())",
                                    "Region" to region(codegenContext.runtimeConfig).resolve("Region"),
                                )
                            }

                        else -> null
                    }
                }

                override fun setBuiltInOnServiceConfig(
                    name: String,
                    value: Node,
                    configBuilderRef: String,
                ): Writable? {
                    if (name != AwsBuiltIns.REGION.builtIn.get()) {
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

    override fun extraSections(codegenContext: ClientCodegenContext): List<AdHocCustomization> =
        listOf(
            adhocCustomization<ServiceConfigSection.MergeFromSharedConfig> { section ->
                rustTemplate(
                    """
                    if self.field_never_set::<#{Region}>() {
                        self.set_region(${section.sdkConfig}.region().cloned());
                    }
                    """,
                    "Region" to region(codegenContext.runtimeConfig).resolve("Region"),
                )
            },
        )
}

class RegionProviderConfig(codegenContext: ClientCodegenContext) : ConfigCustomization() {
    private val region = region(codegenContext.runtimeConfig)
    private val moduleUseName = codegenContext.moduleUseName()
    private val codegenScope =
        arrayOf(
            *preludeScope,
            "Region" to configReexport(region.resolve("Region")),
        )

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.ConfigImpl -> {
                    rustTemplate(
                        """
                        /// Returns the AWS region, if it was provided.
                        pub fn region(&self) -> #{Option}<&#{Region}> {
                            self.config.load::<#{Region}>()
                        }
                        """,
                        *codegenScope,
                    )
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
                }

                is ServiceConfig.BuilderFromConfigBag -> {
                    rustTemplate("${section.builder}.set_region(${section.configBag}.load::<#{Region}>().cloned());", *codegenScope)
                }

                else -> emptySection
            }
        }
}

fun region(runtimeConfig: RuntimeConfig) = AwsRuntimeType.awsTypes(runtimeConfig).resolve("region")

/**
 * Test if region is used and configured for a model (and available on a service client).
 *
 * Services that have an endpoint ruleset that references the SDK::Region built in, or
 * that use SigV4, both need a configurable region.
 */
fun usesRegion(codegenContext: ClientCodegenContext) =
    codegenContext.getBuiltIn(AwsBuiltIns.REGION) != null ||
        ServiceIndex.of(codegenContext.model)
            .getEffectiveAuthSchemes(codegenContext.serviceShape).containsKey(SigV4Trait.ID)
