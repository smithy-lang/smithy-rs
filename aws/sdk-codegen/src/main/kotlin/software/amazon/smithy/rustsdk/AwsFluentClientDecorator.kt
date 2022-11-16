/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

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
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.DependencyScope
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.GenericTypeArg
import software.amazon.smithy.rust.codegen.core.rustlang.RustGenerics
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.asType
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rustsdk.AwsRuntimeType.defaultMiddleware

private class Types(runtimeConfig: RuntimeConfig) {
    private val smithyTypesDep = CargoDependency.SmithyTypes(runtimeConfig)
    private val smithyClientDep = CargoDependency.SmithyClient(runtimeConfig)
    private val smithyHttpDep = CargoDependency.SmithyHttp(runtimeConfig)

    val awsTypes = awsTypes(runtimeConfig).asType()
    val smithyClientRetry = RuntimeType("retry", smithyClientDep, "aws_smithy_client")
    val awsSmithyClient = smithyClientDep.asType()

    val connectorSettings = RuntimeType("ConnectorSettings", smithyClientDep, "aws_smithy_client::http_connector")
    val defaultMiddleware = runtimeConfig.defaultMiddleware()
    val dynConnector = RuntimeType("DynConnector", smithyClientDep, "aws_smithy_client::erase")
    val dynMiddleware = RuntimeType("DynMiddleware", smithyClientDep, "aws_smithy_client::erase")
    val retryConfig = RuntimeType("RetryConfig", smithyTypesDep, "aws_smithy_types::retry")
    val smithyConnector = RuntimeType("SmithyConnector", smithyClientDep, "aws_smithy_client::bounds")
    val timeoutConfig = RuntimeType("TimeoutConfig", smithyTypesDep, "aws_smithy_types::timeout")

    val connectorError = RuntimeType("ConnectorError", smithyHttpDep, "aws_smithy_http::result")
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
    ): Writable =
        writable { }

    override fun toRustGenerics() = RustGenerics()
}

class AwsFluentClientDecorator : RustCodegenDecorator<ClientProtocolGenerator, ClientCodegenContext> {
    override val name: String = "FluentClient"

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
            retryClassifier = runtimeConfig.awsHttp().asType().member("retry::AwsResponseRetryClassifier"),
        ).render(rustCrate)
        rustCrate.withNonRootModule(CustomizableOperationGenerator.CUSTOMIZE_MODULE) {
            renderCustomizableOperationSendMethod(runtimeConfig, generics, this)
        }
        rustCrate.withModule(FluentClientGenerator.clientModule) {
            AwsFluentClientExtensions(types).render(this)
        }
        val awsSmithyClient = "aws-smithy-client"
        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("$awsSmithyClient/rustls")))
        rustCrate.mergeFeature(Feature("native-tls", default = false, listOf("$awsSmithyClient/native-tls")))
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
        "aws_smithy_client" to types.awsSmithyClient,
        "aws_types" to types.awsTypes,
        "retry" to types.smithyClientRetry,
    )

    fun render(writer: RustWriter) {
        writer.rustBlockTemplate("impl Client", *codegenScope) {
            rustTemplate(
                """
                /// Creates a new client from an [SDK Config](#{aws_types}::sdk_config::SdkConfig).
                ##[cfg(any(feature = "rustls", feature = "native-tls"))]
                pub fn new(sdk_config: &#{aws_types}::sdk_config::SdkConfig) -> Self {
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

                    let connector = conf.http_connector().and_then(|c| {
                        let timeout_config = conf
                            .timeout_config()
                            .cloned()
                            .unwrap_or_else(#{TimeoutConfig}::disabled);
                        let connector_settings = #{ConnectorSettings}::from_timeout_config(
                            &timeout_config,
                        );
                        c.connector(&connector_settings, conf.sleep_impl())
                    });

                    let builder = #{aws_smithy_client}::Builder::new();
                    let builder = match connector {
                        // Use provided connector
                        Some(c) => builder.connector(c),
                        // Use default connector based on enabled features
                        None => builder.dyn_https_connector(#{ConnectorSettings}::from_timeout_config(&timeout_config)),
                    };
                    let mut builder = builder
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
    private val serviceName = codegenContext.serviceShape.expectTrait<TitleTrait>().value
    private val serviceShape = codegenContext.serviceShape
    private val crateName = codegenContext.moduleUseName()
    private val codegenScope =
        arrayOf("aws_config" to codegenContext.runtimeConfig.awsConfig().copy(scope = DependencyScope.Dev).asType())

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
                        *codegenScope,
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
    val smithyHttp = CargoDependency.SmithyHttp(runtimeConfig).asType()

    val operationGenerics = RustGenerics(GenericTypeArg("O"), GenericTypeArg("Retry"))
    val handleGenerics = generics.toRustGenerics()
    val combinedGenerics = operationGenerics + handleGenerics

    val codegenScope = arrayOf(
        "combined_generics_decl" to combinedGenerics.declaration(),
        "handle_generics_bounds" to handleGenerics.bounds(),
        "SdkSuccess" to smithyHttp.member("result::SdkSuccess"),
        "ClassifyRetry" to smithyHttp.member("retry::ClassifyRetry"),
        "ParseHttpResponse" to smithyHttp.member("response::ParseHttpResponse"),
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
                E: std::error::Error + 'static,
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
