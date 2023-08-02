/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientDocs
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.NoClientGenerics
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.renderCustomizableOperationSend
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.DefaultProtocolTestGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.Attribute
import software.amazon.smithy.rust.codegen.core.rustlang.Feature
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.util.serviceNameOrDefault
import software.amazon.smithy.rustsdk.AwsRuntimeType.defaultMiddleware

private class Types(runtimeConfig: RuntimeConfig) {
    private val smithyClient = RuntimeType.smithyClient(runtimeConfig)
    private val smithyHttp = RuntimeType.smithyHttp(runtimeConfig)
    private val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)

    val awsTypes = AwsRuntimeType.awsTypes(runtimeConfig)
    val connectorError = smithyHttp.resolve("result::ConnectorError")
    val connectorSettings = smithyClient.resolve("http_connector::ConnectorSettings")
    val dynConnector = smithyClient.resolve("erase::DynConnector")
    val dynMiddleware = smithyClient.resolve("erase::DynMiddleware")
    val retryConfig = smithyTypes.resolve("retry::RetryConfig")
    val smithyClientBuilder = smithyClient.resolve("Builder")
    val smithyClientRetry = smithyClient.resolve("retry")
    val smithyConnector = smithyClient.resolve("bounds::SmithyConnector")
    val timeoutConfig = smithyTypes.resolve("timeout::TimeoutConfig")
}

class AwsFluentClientDecorator : ClientCodegenDecorator {
    override val name: String = "FluentClient"

    // Must run after the AwsPresigningDecorator so that the presignable trait is correctly added to operations
    override val order: Byte = (AwsPresigningDecorator.ORDER + 1).toByte()

    override fun extras(codegenContext: ClientCodegenContext, rustCrate: RustCrate) {
        val runtimeConfig = codegenContext.runtimeConfig
        val types = Types(runtimeConfig)
        val generics = NoClientGenerics(runtimeConfig)
        FluentClientGenerator(
            codegenContext,
            reexportSmithyClientBuilder = false,
            generics = generics,
            customizations = listOf(
                AwsPresignedFluentBuilderMethod(codegenContext),
                AwsFluentClientDocs(codegenContext),
            ),
            retryClassifier = AwsRuntimeType.awsHttp(runtimeConfig).resolve("retry::AwsResponseRetryClassifier"),
        ).render(rustCrate, listOf(CustomizableOperationTestHelpers(runtimeConfig)))
        rustCrate.withModule(ClientRustModule.Client.customize) {
            renderCustomizableOperationSend(codegenContext, generics, this)
        }
        rustCrate.withModule(ClientRustModule.client) {
            AwsFluentClientExtensions(codegenContext, types).render(this)
        }
        val awsSmithyClient = "aws-smithy-client"
        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("$awsSmithyClient/rustls")))
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

    override fun protocolTestGenerator(
        codegenContext: ClientCodegenContext,
        baseGenerator: ProtocolTestGenerator,
    ): ProtocolTestGenerator = DefaultProtocolTestGenerator(
        codegenContext,
        baseGenerator.protocolSupport,
        baseGenerator.operationShape,
        renderClientCreation = { params ->
            rust("let mut ${params.configBuilderName} = ${params.configBuilderName};")
            if (codegenContext.smithyRuntimeMode.generateOrchestrator) {
                rust("""${params.configBuilderName}.set_region(Some(crate::config::Region::new("us-east-1")));""")
            } else {
                rust(
                    """
                    // If the test case was missing endpoint parameters, default a region so it doesn't fail
                    if ${params.configBuilderName}.region.is_none() {
                        ${params.configBuilderName}.set_region(Some(crate::config::Region::new("us-east-1")));
                    }
                    """,
                )
            }
            rustTemplate(
                """
                let config = ${params.configBuilderName}.http_connector(${params.connectorName}).build();
                let ${params.clientName} = #{Client}::from_conf(config);
                """,
                "Client" to ClientRustModule.root.toType().resolve("Client"),
            )
        },
    )
}

private class AwsFluentClientExtensions(private val codegenContext: ClientCodegenContext, private val types: Types) {
    private val codegenScope = arrayOf(
        "Arc" to RuntimeType.Arc,
        "ConnectorError" to types.connectorError,
        "ConnectorSettings" to types.connectorSettings,
        "DynConnector" to types.dynConnector,
        "DynMiddleware" to types.dynMiddleware,
        "RetryConfig" to types.retryConfig,
        "SmithyConnector" to types.smithyConnector,
        "TimeoutConfig" to types.timeoutConfig,
        "SmithyClientBuilder" to types.smithyClientBuilder,
        "aws_types" to types.awsTypes,
        "retry" to types.smithyClientRetry,
    )

    fun render(writer: RustWriter) {
        writer.rustBlockTemplate("impl Client", *codegenScope) {
            rustTemplate(
                """
                /// Creates a new client from an [SDK Config](#{aws_types}::sdk_config::SdkConfig).
                ///
                /// ## Panics
                ///
                /// - This method will panic if the `sdk_config` is missing an async sleep implementation. If you experience this panic, set
                ///     the `sleep_impl` on the Config passed into this function to fix it.
                /// - This method will panic if the `sdk_config` is missing an HTTP connector. If you experience this panic, set the
                ///     `http_connector` on the Config passed into this function to fix it.
                pub fn new(sdk_config: &#{aws_types}::sdk_config::SdkConfig) -> Self {
                    Self::from_conf(sdk_config.into())
                }
                """,
                *codegenScope,
            )
            if (codegenContext.smithyRuntimeMode.generateMiddleware) {
                rustTemplate(
                    """
                    /// Creates a new client from the service [`Config`](crate::Config).
                    ///
                    /// ## Panics
                    ///
                    /// - This method will panic if the `conf` is missing an async sleep implementation. If you experience this panic, set
                    ///     the `sleep_impl` on the Config passed into this function to fix it.
                    /// - This method will panic if the `conf` is missing an HTTP connector. If you experience this panic, set the
                    ///     `http_connector` on the Config passed into this function to fix it.
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

                        let builder = #{SmithyClientBuilder}::new();

                        let builder = match connector {
                            // Use provided connector
                            Some(c) => builder.connector(c),
                            None =>{
                                ##[cfg(feature = "rustls")]
                                {
                                    // Use default connector based on enabled features
                                    builder.dyn_https_connector(#{ConnectorSettings}::from_timeout_config(&timeout_config))
                                }
                                ##[cfg(not(feature = "rustls"))]
                                {
                                    panic!("No HTTP connector was available. Enable the `rustls` crate feature or set a connector to fix this.");
                                }
                            }
                        };
                        let mut builder = builder
                            .middleware(#{DynMiddleware}::new(#{Middleware}::new()))
                            .reconnect_mode(retry_config.reconnect_mode())
                            .retry_config(retry_config.into())
                            .operation_timeout_config(timeout_config.into());
                        builder.set_sleep_impl(sleep_impl);
                        let client = builder.build();

                        Self { handle: #{Arc}::new(Handle { client, conf }) }
                    }
                    """,
                    *codegenScope,
                    "Middleware" to codegenContext.runtimeConfig.defaultMiddleware(),
                )
            }
        }
    }
}

private class AwsFluentClientDocs(private val codegenContext: ClientCodegenContext) : FluentClientCustomization() {
    private val serviceName = codegenContext.serviceShape.serviceNameOrDefault("the service")

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
                AwsDocs.clientConstructionDocs(codegenContext)(this)
                FluentClientDocs.clientUsageDocs(codegenContext)(this)
            }

            else -> emptySection
        }
    }
}
