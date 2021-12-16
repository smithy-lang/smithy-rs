/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.TitleTrait
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.rustlang.Feature
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustModule
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustBlockTemplate
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
import software.amazon.smithy.rustsdk.AwsRuntimeType.defaultMiddleware

private class Types(runtimeConfig: RuntimeConfig) {
    private val smithyClientDep = CargoDependency.SmithyClient(runtimeConfig)

    val awsTypes = awsTypes(runtimeConfig).asType()
    val smithyClientRetry = RuntimeType("retry", smithyClientDep, "aws_smithy_client")
    val awsSmithyClient = smithyClientDep.asType()

    val defaultMiddleware = runtimeConfig.defaultMiddleware()
    val dynConnector = RuntimeType("DynConnector", smithyClientDep, "aws_smithy_client::erase")
}

class AwsFluentClientDecorator : RustCodegenDecorator {
    override val name: String = "FluentClient"

    // Must run after the AwsPresigningDecorator so that the presignable trait is correctly added to operations
    override val order: Byte = (AwsPresigningDecorator.ORDER + 1).toByte()

    override fun extras(codegenContext: CodegenContext, rustCrate: RustCrate) {
        val types = Types(codegenContext.runtimeConfig)
        val module = RustMetadata(public = true)
        rustCrate.withModule(
            RustModule(
                "client",
                module,
                documentation = "Client and fluent builders for calling the service."
            )
        ) { writer ->
            FluentClientGenerator(
                codegenContext,
                generics = ClientGenerics(
                    connectorDefault = types.dynConnector,
                    middlewareDefault = types.defaultMiddleware,
                    retryDefault = types.smithyClientRetry.member("Standard"),
                    client = types.awsSmithyClient
                ),
                customizations = listOf(
                    AwsPresignedFluentBuilderMethod(codegenContext.runtimeConfig),
                    AwsFluentClientDocs(codegenContext)
                )
            ).render(writer)
            AwsFluentClientExtensions(types).render(writer)
        }
        val awsSmithyClient = "aws-smithy-client"
        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("$awsSmithyClient/rustls")))
        rustCrate.mergeFeature(Feature("native-tls", default = false, listOf("$awsSmithyClient/native-tls")))
    }

    override fun libRsCustomizations(
        codegenContext: CodegenContext,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations + object : LibRsCustomization() {
            override fun section(section: LibRsSection) = when (section) {
                is LibRsSection.Body -> writable {
                    rust("pub use client::Client;")
                }
                else -> emptySection
            }
        }
    }
}

private class AwsFluentClientExtensions(private val types: Types) {
    val clientGenerics = ClientGenerics(
        connectorDefault = types.dynConnector,
        middlewareDefault = types.defaultMiddleware,
        retryDefault = types.smithyClientRetry.member("Standard"),
        client = types.awsSmithyClient
    )

    private val codegenScope = arrayOf(
        "Middleware" to types.defaultMiddleware,
        "retry" to types.smithyClientRetry,
        "DynConnector" to types.dynConnector,
        "aws_smithy_client" to types.awsSmithyClient
    )

    fun render(writer: RustWriter) {
        writer.rustBlockTemplate("impl<C> Client<C, #{Middleware}, #{retry}::Standard>", *codegenScope) {
            rustTemplate(
                """
                /// Creates a client with the given service config and connector override.
                pub fn from_conf_conn(conf: crate::Config, conn: C) -> Self {
                    let retry_config = conf.retry_config.as_ref().cloned().unwrap_or_default();
                    let timeout_config = conf.timeout_config.as_ref().cloned().unwrap_or_default();
                    let sleep_impl = conf.sleep_impl.clone();
                    let mut builder = #{aws_smithy_client}::Builder::new()
                        .connector(conn)
                        .middleware(#{Middleware}::new());
                    builder.set_retry_config(retry_config.into());
                    builder.set_timeout_config(timeout_config);
                    if let Some(sleep_impl) = sleep_impl {
                        builder.set_sleep_impl(Some(sleep_impl));
                    }
                    let client = builder.build();
                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }
                """,
                *codegenScope
            )
        }
        writer.rustBlockTemplate("impl Client<#{DynConnector}, #{Middleware}, #{retry}::Standard>", *codegenScope) {
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
                    let mut builder = #{aws_smithy_client}::Builder::dyn_https()
                        .middleware(#{Middleware}::new());
                    builder.set_retry_config(retry_config.into());
                    builder.set_timeout_config(timeout_config);
                    // the builder maintains a try-state. To avoid suppressing the warning when sleep is unset,
                    // only set it if we actually have a sleep impl.
                    if let Some(sleep_impl) = sleep_impl {
                        builder.set_sleep_impl(Some(sleep_impl));
                    }
                    let client = builder.build();

                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }
                """,
                "aws_smithy_client" to types.awsSmithyClient,
                "aws_types" to types.awsTypes,
                "Middleware" to types.defaultMiddleware
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
                        ///         .<operation_name>().
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
