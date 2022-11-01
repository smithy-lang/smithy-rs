/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.decorators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.TitleTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.CustomizableOperationGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerics
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.GenericTypeArg
import software.amazon.smithy.rust.codegen.core.rustlang.RustGenerics
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.smithyClient
import software.amazon.smithy.rust.codegen.core.rustlang.smithyHttp
import software.amazon.smithy.rust.codegen.core.rustlang.smithyTypes
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rustsdk.AwsRuntimeType.defaultMiddleware
import software.amazon.smithy.rustsdk.SdkSettings
import software.amazon.smithy.rustsdk.awsConfig
import software.amazon.smithy.rustsdk.awsHttp
import software.amazon.smithy.rustsdk.awsTypes

private class Types(runtimeConfig: RuntimeConfig) {
    val clientBuilder = runtimeConfig.smithyClient().member("Builder")
    val connectorError = runtimeConfig.smithyHttp().member("result::ConnectorError")
    val connectorSettings = runtimeConfig.smithyClient().member("http_connector::ConnectorSettings")
    val defaultMiddleware = runtimeConfig.defaultMiddleware()
    val dynConnector = runtimeConfig.smithyClient().member("erase::DynConnector")
    val dynMiddleware = runtimeConfig.smithyClient().member("erase::DynMiddleware")
    val retryConfig = runtimeConfig.smithyTypes().member("retry::RetryConfig")
    val sdkConfig = runtimeConfig.awsTypes().member("SdkConfig")
    val smithyClientRetry = runtimeConfig.smithyClient().member("retry")
    val smithyConnector = runtimeConfig.smithyClient().member("bounds::SmithyConnector")
    val timeoutConfig = runtimeConfig.smithyTypes().member("timeout::TimeoutConfig")
}

private class AwsClientGenerics(private val types: Types) : FluentClientGenerics {
    /** Declaration with defaults set */
    override val decl = writable { }

    /** Instantiation of the Smithy client generics */
    override val smithyInst = writable {
        rustTemplate(
            "<#{DynConnector}, #{DynMiddleware}<#{DynConnector}>>",
            "DynConnector" to types.dynConnector,
            "DynMiddleware" to types.dynMiddleware,
        )
    }

    /** Instantiation */
    override val inst = ""

    /** Trait bounds */
    override val bounds = writable { }

    /** Bounds for generated `send()` functions */
    override fun sendBounds(
        operation: Symbol,
        operationOutput: Symbol,
        operationError: RuntimeType,
        retryClassifier: RuntimeType,
    ): Writable = writable { }

    override fun toRustGenerics() = RustGenerics()
}

class AwsFluentClientDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "AwsFluentClient"

    // Must run after the AwsPresigningDecorator so that the presignable trait is correctly added to operations
    override val order: Byte = (AwsPresigningDecorator.ORDER + 1).toByte()

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val runtimeConfig = codegenContext.runtimeConfig
        val types = Types(runtimeConfig)
        val generics = AwsClientGenerics(types)
        FluentClientGenerator(
            codegenContext,
            generics,
            customizations = listOf(
                AwsPresignedFluentBuilderMethod(runtimeConfig),
                AwsFluentClientDocs(codegenContext),
            ),
            retryClassifier = runtimeConfig.awsHttp().member("retry::AwsResponseRetryClassifier"),
        ).render(rustCrate)
        rustCrate.withNonRootModule(CustomizableOperationGenerator.CUSTOMIZE_MODULE) {
            renderCustomizableOperationSendMethod(runtimeConfig, generics, this)
        }
        rustCrate.withModule(FluentClientGenerator.clientModule) {
            AwsFluentClientExtensions(types).render(this)
        }
        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("aws-smithy-client/rustls")))
        rustCrate.mergeFeature(Feature("native-tls", default = false, listOf("aws-smithy-client/native-tls")))
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations + object : LibRsCustomization() {
            override fun section(section: LibRsSection) = when (section) {
                is LibRsSection.Body -> writable {
                    Attribute.DocInline.render(this)
                    rust("pub use client::Client;")
                }

                else -> emptySection
            }
        }
    }

    override fun supportsCodegenContext(clazz: Class<out CodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

private class AwsFluentClientExtensions(types: Types) {
    private val codegenScope = arrayOf(
        "ConnectorError" to types.connectorError,
        "DynConnector" to types.dynConnector,
        "DynMiddleware" to types.dynMiddleware,
        "ConnectorSettings" to types.connectorSettings,
        "Middleware" to types.defaultMiddleware,
        "RetryConfig" to types.retryConfig,
        "SmithyConnector" to types.smithyConnector,
        "TimeoutConfig" to types.timeoutConfig,
        "ClientBuilder" to types.clientBuilder,
        "SdkConfig" to types.sdkConfig,
        "retry" to types.smithyClientRetry,
    )

    fun render(writer: RustWriter) {
        writer.rustBlockTemplate("impl Client", *codegenScope) {
            rustTemplate(
                """
                /// Creates a client with the given service config and connector override.
                pub fn from_conf_conn<C, E>(conf: crate::Config, conn: C) -> Self
                where
                    C: #{SmithyConnector}<Error = E> + Send + 'static,
                    E: Into<#{ConnectorError}>,
                {
                    let retry_config = conf.retry_config().cloned().unwrap_or_else(#{RetryConfig}::disabled);
                    let timeout_config = conf.timeout_config().cloned().unwrap_or_else(#{TimeoutConfig}::disabled);
                    let mut builder = #{ClientBuilder}::new()
                        .connector(#{DynConnector}::new(conn))
                        .middleware(#{DynMiddleware}::new(#{Middleware}::new()))
                        .retry_config(retry_config.into())
                        .operation_timeout_config(timeout_config.into());
                    builder.set_sleep_impl(conf.sleep_impl());
                    let client = builder.build();
                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }

                /// Creates a new client from a shared config.
                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn new(sdk_config: &#{SdkConfig}) -> Self {
                    Self::from_conf(sdk_config.into())
                }

                /// Creates a new client from the service [`Config`](crate::Config).
                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn from_conf(conf: crate::Config) -> Self {
                    let retry_config = conf.retry_config().cloned().unwrap_or_else(#{RetryConfig}::disabled);
                    let timeout_config = conf.timeout_config().cloned().unwrap_or_else(#{TimeoutConfig}::disabled);
                    let sleep_impl = conf.sleep_impl();
                    if (retry_config.has_retry() || timeout_config.has_timeouts()) && sleep_impl.is_none() {
                        panic!("An async sleep implementation is required for retries or timeouts to work. \
                                Set the `sleep_impl` on the Config passed into this function to fix this panic.");
                    }
                    let mut builder = #{ClientBuilder}::new()
                        .dyn_https_connector(#{ConnectorSettings}::from_timeout_config(&timeout_config))
                        .middleware(#{DynMiddleware}::new(#{Middleware}::new()))
                        .retry_config(retry_config.into())
                        .operation_timeout_config(timeout_config.into());
                    builder.set_sleep_impl(sleep_impl);
                    let client = builder.build();

                    Self { handle: std::sync::Arc::new(Handle { client, conf }) }
                }
                """,
                *codegenScope,
            )
        }
    }
}

private class AwsFluentClientDocs(private val codegenContext: CodegenContext) : FluentClientCustomization() {
    private val serviceShape = codegenContext.serviceShape
    private val serviceName = serviceShape.expectTrait<TitleTrait>().value
    private val crateName = codegenContext.moduleUseName()
    private val runtimeConfig = codegenContext.runtimeConfig

    // If no `aws-config` version is provided, assume that docs referencing `aws-config` cannot be given.
    // Also, STS and SSO must NOT reference `aws-config` since that would create a circular dependency.
    private fun suppressUsageDocs(): Boolean =
        SdkSettings.from(codegenContext.settings).awsConfigVersion == null ||
            setOf(
                ShapeId.from("com.amazonaws.sts#AWSSecurityTokenServiceV20110615"),
                ShapeId.from("com.amazonaws.sso#SWBPortalService"),
            ).contains(serviceShape.id)

    override fun section(section: FluentClientSection): Writable {
        return when (section) {
            is FluentClientSection.FluentClientDocs -> writable {
                rustTemplate(
                    """
                    /// Client for $serviceName
                    ///
                    /// Client for invoking operations on $serviceName. Each operation on $serviceName is a method on this
                    /// this struct. `.send()` MUST be invoked on the generated operations to dispatch the request to the service.""",
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
                        /// use #{aws_config}::retry::RetryConfig;
                        /// ## async fn docs() {
                        /// let shared_config = #{aws_config}::load_from_env().await;
                        /// let config = $crateName::config::Builder::from(&shared_config)
                        ///   .retry_config(RetryConfig::disabled())
                        ///   .build();
                        /// let client = $crateName::Client::from_conf(config);
                        /// ## }
                        """,
                        "aws_config" to runtimeConfig.awsConfig(),
                    )
                }
            }

            else -> emptySection
        }
    }
}

private fun renderCustomizableOperationSendMethod(
    runtimeConfig: RuntimeConfig,
    generics: FluentClientGenerics,
    writer: RustWriter,
) {
    val operationGenerics = RustGenerics(GenericTypeArg("O"), GenericTypeArg("Retry"))
    val handleGenerics = generics.toRustGenerics()
    val combinedGenerics = operationGenerics + handleGenerics

    val codegenScope = arrayOf(
        "combined_generics_decl" to combinedGenerics.declaration(),
        "handle_generics_bounds" to handleGenerics.bounds(),
        "SdkSuccess" to runtimeConfig.smithyHttp().member("result::SdkSuccess"),
        "ClassifyRetry" to runtimeConfig.smithyHttp().member("retry::ClassifyRetry"),
        "ParseHttpResponse" to runtimeConfig.smithyHttp().member("response::ParseHttpResponse"),
    )

    writer.rustTemplate(
        """
        impl#{combined_generics_decl:W} CustomizableOperation#{combined_generics_decl:W}
        where
            #{handle_generics_bounds:W}
        {
            /// Sends this operation's request
            pub async fn send<T, E>(self) -> Result<T, SdkError<E>>
            where
                E: std::error::Error,
                O: #{ParseHttpResponse}<Output = Result<T, E>> + Send + Sync + Clone + 'static,
                Retry: #{ClassifyRetry}<#{SdkSuccess}<T>, SdkError<E>> + Send + Sync + Clone,
            {
                self.handle.client.call(self.operation).await
            }
        }
        """,
        *codegenScope,
    )
}
