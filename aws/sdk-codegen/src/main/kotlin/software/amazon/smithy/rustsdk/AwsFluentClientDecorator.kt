/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustCrate
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ClientGenerics
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection

private class Types(runtimeConfig: RuntimeConfig) {
    private val smithyClientDep = CargoDependency.SmithyClient(runtimeConfig).copy(optional = true)
    private val awsHyperDep = runtimeConfig.awsRuntimeDependency("aws-hyper").copy(optional = true)

    val awsTypes = awsTypes(runtimeConfig).asType()
    val awsHyper = awsHyperDep.asType()
    val smithyClientRetry = RuntimeType("retry", smithyClientDep, "smithy_client")

    val AwsMiddleware = RuntimeType("AwsMiddleware", awsHyperDep, "aws_hyper")
    val DynConnector = RuntimeType("DynConnector", smithyClientDep, "smithy_client::erase")
}

class AwsFluentClientDecorator : RustCodegenDecorator {
    override val name: String = "FluentClient"

    // Must run after the AwsPresigningDecorator so that the presignable trait is correctly added to operations
    override val order: Byte = (AwsPresigningDecorator.ORDER + 1).toByte()

    override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        val types = Types(codegenContext.runtimeConfig)
        val module = RustMetadata(additionalAttributes = listOf(Attribute.Cfg.feature("client")), public = true)
        rustCrate.withModule(RustModule("client", module)) { writer ->
            FluentClientGenerator(
                codegenContext,
                includeSmithyGenericClientDocs = false,
                generics = ClientGenerics(
                    connectorDefault = "#{AwsFluentClient_DynConnector}",
                    middlewareDefault = "#{AwsFluentClient_AwsMiddleware}",
                    retryDefault = "#{AwsFluentClient_retry}::Standard",
                    codegenScope = listOf(
                        "AwsFluentClient_AwsMiddleware" to types.AwsMiddleware,
                        "AwsFluentClient_DynConnector" to types.DynConnector,
                        "AwsFluentClient_retry" to types.smithyClientRetry,
                    )
                ),
                customizations = listOf(AwsPresignedFluentBuilderMethod(codegenContext.runtimeConfig))
            ).render(writer)
            AwsFluentClientExtensions(types).render(writer)
        }
        val awsHyper = "aws-hyper"
        rustCrate.mergeFeature(Feature("client", default = true, listOf(awsHyper, "smithy-client")))
        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("$awsHyper/rustls")))
        rustCrate.mergeFeature(Feature("native-tls", default = false, listOf("$awsHyper/native-tls")))
    }

    override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + object : LibRsCustomization() {
            override fun section(section: LibRsSection) = when (section) {
                is LibRsSection.Body -> writable {
                    Attribute.Cfg.feature("client").render(this)
                    rust("pub use client::Client;")
                }
                else -> emptySection
            }
        }
    }
}

private class AwsFluentClientExtensions(private val types: Types) {
    fun render(writer: RustWriter) {
        writer.rustBlock("impl<C> Client<C, aws_hyper::AwsMiddleware, smithy_client::retry::Standard>") {
            rustTemplate(
                """
                pub fn from_conf_conn(conf: crate::Config, conn: C) -> Self {
                    let retry_config = conf.retry_config.as_ref().cloned().unwrap_or_default();
                    let client = #{aws_hyper}::Client::new(conn).with_retry_config(retry_config.into());
                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }
                """,
                "aws_hyper" to types.awsHyper,
            )
        }
        writer.rustBlock("impl Client<smithy_client::erase::DynConnector, aws_hyper::AwsMiddleware, smithy_client::retry::Standard>") {
            rustTemplate(
                """
                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn new(config: &#{aws_types}::config::Config) -> Self {
                    Self::from_conf(config.into())
                }

                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn from_conf(conf: crate::Config) -> Self {
                    let retry_config = conf.retry_config.as_ref().cloned().unwrap_or_default();
                    let client = #{aws_hyper}::Client::https().with_retry_config(retry_config.into());
                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }
                """,
                "aws_hyper" to types.awsHyper,
                "aws_types" to types.awsTypes
            )
        }
    }
}
