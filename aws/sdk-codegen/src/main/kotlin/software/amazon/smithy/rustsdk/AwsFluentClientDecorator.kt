/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.TitleTrait
import software.amazon.smithy.rust.codegen.rustlang.Attribute
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
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
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientGenerator
import software.amazon.smithy.rust.codegen.smithy.generators.FluentClientSection
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.util.expectTrait

private class Types(runtimeConfig: RuntimeConfig) {
    private val smithyClientDep = CargoDependency.SmithyClient(runtimeConfig).copy(optional = true)
    private val awsHyperDep = runtimeConfig.awsRuntimeDependency("aws-hyper").copy(optional = true)

    val awsTypes = awsTypes(runtimeConfig).asType()
    val awsHyper = awsHyperDep.asType()
    val smithyClientRetry = RuntimeType("retry", smithyClientDep, "aws_smithy_client")

    val awsMiddleware = RuntimeType("AwsMiddleware", awsHyperDep, "aws_hyper")
    val dynConnector = RuntimeType("DynConnector", smithyClientDep, "aws_smithy_client::erase")
}

class AwsFluentClientDecorator : RustCodegenDecorator {
    override val name: String = "FluentClient"

    // Must run after the AwsPresigningDecorator so that the presignable trait is correctly added to operations
    override val order: Byte = (AwsPresigningDecorator.ORDER + 1).toByte()

    override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        val types = Types(codegenContext.runtimeConfig)
        val module = RustMetadata(additionalAttributes = listOf(Attribute.Cfg.feature("client")), public = true)
        rustCrate.withModule(
            RustModule(
                "client",
                module,
                documentation = "Client and fluent builders for calling the service."
            )
        ) { writer ->
            FluentClientGenerator(
                codegenContext,
                includeSmithyGenericClientDocs = false,
                generics = ClientGenerics(
                    connectorDefault = "#{AwsFluentClient_DynConnector}",
                    middlewareDefault = "#{AwsFluentClient_AwsMiddleware}",
                    retryDefault = "#{AwsFluentClient_retry}::Standard",
                    codegenScope = listOf(
                        "AwsFluentClient_AwsMiddleware" to types.awsMiddleware,
                        "AwsFluentClient_DynConnector" to types.dynConnector,
                        "AwsFluentClient_retry" to types.smithyClientRetry,
                    )
                ),
                customizations = listOf(
                    AwsPresignedFluentBuilderMethod(codegenContext.runtimeConfig),
                    AwsFluentClientDocs(codegenContext)
                )
            ).render(writer)
            AwsFluentClientExtensions(types).render(writer)
        }
        val awsHyper = "aws-hyper"
        val awsSmithyClient = "aws-smithy-client"
        rustCrate.mergeFeature(Feature("client", default = true, listOf(awsSmithyClient, awsHyper)))
        rustCrate.mergeFeature(Feature("rustls", default = false, listOf("$awsHyper/rustls", "$awsSmithyClient/rustls")))
        rustCrate.mergeFeature(Feature("native-tls", default = false, listOf("$awsHyper/native-tls", "$awsSmithyClient/native-tls")))
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
        writer.rustBlock("impl<C> Client<C, aws_hyper::AwsMiddleware, aws_smithy_client::retry::Standard>") {
            rustTemplate(
                """
                /// Creates a client with the given service config and connector override.
                pub fn from_conf_conn(conf: crate::Config, conn: C) -> Self {
                    let retry_config = conf.retry_config.as_ref().cloned().unwrap_or_default();
                    let timeout_config = conf.timeout_config.as_ref().cloned().unwrap_or_default();
                    let sleep_impl = conf.sleep_impl.clone();
                    let mut client = #{aws_hyper}::Client::new(conn)
                        .with_retry_config(retry_config.into())
                        .with_timeout_config(timeout_config);

                    client.set_sleep_impl(sleep_impl);
                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }
                """,
                "aws_hyper" to types.awsHyper,
            )
        }
        writer.rustBlock("impl Client<aws_smithy_client::erase::DynConnector, aws_hyper::AwsMiddleware, aws_smithy_client::retry::Standard>") {
            rustTemplate(
                """
                /// Creates a new client from a shared config.
                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn new(config: &#{aws_types}::config::Config) -> Self {
                    Self::from_conf(config.into())
                }

                /// Creates a new client from the service [`Config`](crate::Config).
                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn from_conf(conf: crate::Config) -> Self {
                    let retry_config = conf.retry_config.as_ref().cloned().unwrap_or_default();
                    let timeout_config = conf.timeout_config.as_ref().cloned().unwrap_or_default();
                    let sleep_impl = conf.sleep_impl.clone();
                    let mut client = #{aws_hyper}::Client::https()
                        .with_retry_config(retry_config.into())
                        .with_timeout_config(timeout_config);

                    client.set_sleep_impl(sleep_impl);
                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }
                """,
                "aws_hyper" to types.awsHyper,
                "aws_types" to types.awsTypes
            )
        }
    }
}

private class AwsFluentClientDocs(codegenContext: CodegenContext) : FluentClientCustomization() {
    private val serviceName = codegenContext.serviceShape.expectTrait<TitleTrait>().value
    private val serviceShape = codegenContext.serviceShape
    private val crateName = codegenContext.moduleUseName()
    private val codegenScope =
        arrayOf("aws_config" to codegenContext.runtimeConfig.awsConfig().copy(scope = DependencyScope.Dev).asType())

    // Usage docs on STS must be suppressed—aws-config cannot be added as a dev-dependency because it would create
    // a circular dependency
    private fun suppressUsageDocs(): Boolean =
        serviceShape.id == ShapeId.from("com.amazonaws.sts#AWSSecurityTokenServiceV20110615")

    override fun section(section: FluentClientSection): Writable {
        return when (section) {
            is FluentClientSection.FluentClientDocs -> writable {
                rustTemplate(
                    """
                    /// Client for $serviceName
                    ///
                    /// Client for invoking operations on $serviceName. Each operation on $serviceName is a method on this
                    /// this struct. `.send()` MUST be invoked on the generated operations to dispatch the request to the service."""
                )
                if (!suppressUsageDocs()) {
                    rustTemplate(
                        """
                        ///
                        /// ## Examples
                        /// **Constructing a client and invoking an operation**
                        /// ```rust,no_run
                        /// ## async fn docs() {
                        ///     // create a shared configuration. This can be used & shared between multiple service clients.
                        ///     let shared_config = #{aws_config}::load_from_env().await;
                        ///     let client = $crateName::Client::new(&shared_config);
                        ///     // invoke an operation
                        ///     /* let rsp = client
                        ///         .<operationname>().
                        ///         .<param>("some value")
                        ///         .send().await; */
                        /// ## }
                        /// ```
                        /// **Constructing a client with custom configuration**
                        /// ```rust,no_run
                        /// use #{aws_config}::RetryConfig;
                        /// ## async fn docs() {
                        ///     let shared_config = #{aws_config}::load_from_env().await;
                        ///     let config = $crateName::config::Builder::from(&shared_config)
                        ///         .retry_config(RetryConfig::disabled())
                        ///         .build();
                        ///     let client = $crateName::Client::from_conf(config);
                        /// ## }
                        """,
                        *codegenScope
                    )
                }
            }
            else -> emptySection
        }
    }
}
