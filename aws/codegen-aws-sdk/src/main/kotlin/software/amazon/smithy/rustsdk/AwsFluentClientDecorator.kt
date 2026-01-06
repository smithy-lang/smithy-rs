/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientDocs
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientGenerator
import software.amazon.smithy.rust.codegen.client.smithy.generators.client.FluentClientSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.protocol.ClientProtocolTestGenerator
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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.RustCrate
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.core.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.protocol.ProtocolTestGenerator
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.serviceNameOrDefault
import software.amazon.smithy.rustsdk.customize.s3.S3ExpressFluentClientCustomization

private class Types(runtimeConfig: RuntimeConfig) {
    private val smithyTypes = RuntimeType.smithyTypes(runtimeConfig)

    val awsTypes = AwsRuntimeType.awsTypes(runtimeConfig)
    val retryConfig = smithyTypes.resolve("retry::RetryConfig")
    val timeoutConfig = smithyTypes.resolve("timeout::TimeoutConfig")
}

class AwsFluentClientDecorator : ClientCodegenDecorator {
    override val name: String = "FluentClient"

    // Must run after the AwsPresigningDecorator so that the presignable trait is correctly added to operations
    override val order: Byte = (AwsPresigningDecorator.ORDER + 1).toByte()

    override fun extras(
        codegenContext: ClientCodegenContext,
        rustCrate: RustCrate,
    ) {
        val runtimeConfig = codegenContext.runtimeConfig
        val types = Types(runtimeConfig)
        FluentClientGenerator(
            codegenContext,
            customizations =
                listOf(
                    AwsPresignedFluentBuilderMethod(codegenContext),
                    AwsFluentClientDocs(codegenContext),
                    AwsFluentClientRetryPartition(codegenContext),
                    AwsFluentClientEnableRetries(codegenContext), // NEW: Enable retries for AWS SDK
                ).letIf(codegenContext.serviceShape.id == ShapeId.from("com.amazonaws.s3#AmazonS3")) {
                    it + S3ExpressFluentClientCustomization(codegenContext)
                },
        ).render(rustCrate)
        rustCrate.withModule(ClientRustModule.client) {
            AwsFluentClientExtensions(codegenContext, types).render(this)
        }

        // TODO(hyper1): disable rustls as a default feature in future release
        // NOTE: We enable both rustls and default-https-client as default features. This keeps the legacy hyper+rustls
        // stack working as is and lets BehaviorVersion control which client you get. In a future release we will
        // break this and disable the rustls feature by default (and break old BMV versions w.r.t http client default).
        rustCrate.mergeFeature(Feature("rustls", default = true, listOf("aws-smithy-runtime/tls-rustls")))
        rustCrate.mergeFeature(Feature("default-https-client", default = true, listOf("aws-smithy-runtime/default-https-client")))
    }

    override fun libRsCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<LibRsCustomization>,
    ): List<LibRsCustomization> {
        return baseCustomizations +
            object : LibRsCustomization() {
                override fun section(section: LibRsSection) =
                    when (section) {
                        is LibRsSection.Body ->
                            writable {
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
    ): ProtocolTestGenerator =
        ClientProtocolTestGenerator(
            codegenContext,
            baseGenerator.protocolSupport,
            baseGenerator.operationShape,
            renderClientCreation = { params ->
                rustTemplate(
                    """
                    let mut ${params.configBuilderName} = ${params.configBuilderName};
                    ${params.configBuilderName}.set_region(Some(crate::config::Region::new("us-east-1")));

                    let config = ${params.configBuilderName}.http_client(${params.httpClientName}).build();
                    let ${params.clientName} = #{Client}::from_conf(config);
                    """,
                    "Client" to ClientRustModule.root.toType().resolve("Client"),
                )
            },
        )
}

private class AwsFluentClientExtensions(private val codegenContext: ClientCodegenContext, private val types: Types) {
    private val codegenScope =
        arrayOf(
            "Arc" to RuntimeType.Arc,
            "RetryConfig" to types.retryConfig,
            "TimeoutConfig" to types.timeoutConfig,
            "aws_types" to types.awsTypes,
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
                ///   the `sleep_impl` on the Config passed into this function to fix it.
                /// - This method will panic if the `sdk_config` is missing an HTTP connector. If you experience this panic, set the
                ///   `http_connector` on the Config passed into this function to fix it.
                /// - This method will panic if no `BehaviorVersion` is provided. If you experience this panic, set `behavior_version` on the Config or enable the `behavior-version-latest` Cargo feature.
                ##[track_caller]
                pub fn new(sdk_config: &#{aws_types}::sdk_config::SdkConfig) -> Self {
                    Self::from_conf(sdk_config.into())
                }
                """,
                *codegenScope,
            )
        }
    }
}

private class AwsFluentClientDocs(private val codegenContext: ClientCodegenContext) : FluentClientCustomization() {
    private val serviceName = codegenContext.serviceShape.serviceNameOrDefault("the service")

    override fun section(section: FluentClientSection): Writable {
        return when (section) {
            is FluentClientSection.FluentClientDocs ->
                writable {
                    rustTemplate(
                        """
                        /// Client for $serviceName
                        ///
                        /// Client for invoking operations on $serviceName. Each operation on $serviceName is a method on this
                        /// this struct. `.send()` MUST be invoked on the generated operations to dispatch the request to the service.""",
                    )
                    AwsDocs.clientConstructionDocs(codegenContext)(this)
                    FluentClientDocs.clientUsageDocs(codegenContext)(this)
                    FluentClientDocs.waiterDocs(codegenContext)(this)
                }

            else -> emptySection
        }
    }
}

/**
 * Replaces the default retry partition for all operations to include the AWS region if set
 */
private class AwsFluentClientRetryPartition(private val codegenContext: ClientCodegenContext) : FluentClientCustomization() {
    override fun section(section: FluentClientSection): Writable {
        return when {
            section is FluentClientSection.BeforeBaseClientPluginSetup && usesRegion(codegenContext) -> {
                writable {
                    rustTemplate(
                        """
                        let default_retry_partition = match config.region() {
                            Some(region) => #{Cow}::from(format!("{default_retry_partition}-{region}")),
                            None => #{Cow}::from(default_retry_partition),
                        };
                        """,
                        *preludeScope,
                        "Cow" to RuntimeType.Cow,
                    )
                }
            }
            else -> emptySection
        }
    }
}

/**
 * Enables retries by default for AWS SDK clients when awsSdkBuild is true
 */
private class AwsFluentClientEnableRetries(private val codegenContext: ClientCodegenContext) : FluentClientCustomization() {
    override fun section(section: FluentClientSection): Writable {
        // Only enable for real AWS SDK builds (controlled by awsSdkBuild in JSON settings)
        if (!codegenContext.sdkSettings().awsSdkBuild) {
            return emptySection
        }
        
        return when (section) {
            is FluentClientSection.CustomizeDefaultPluginParams -> {
                writable {
                    rust(".with_is_aws_sdk(true)")
                }
            }
            else -> emptySection
        }
    }
}
