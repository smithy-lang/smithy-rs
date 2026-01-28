/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.model.traits.HttpHeaderTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ConfigCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.config.ServiceConfig
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember

class AwsChunkedContentEncodingDecorator : ClientCodegenDecorator {
    override val name: String = "AwsChunkedContentEncoding"

    // This decorator must decorate after any of the following:
    // - HttpRequestChecksumDecorator
    // - HttpRequestCompressionDecorator
    override val order: Byte =
        (minOf(HttpRequestChecksumDecorator.ORDER, HttpRequestCompressionDecorator.ORDER) - 1).toByte()

    override fun configCustomizations(
        codegenContext: ClientCodegenContext,
        baseCustomizations: List<ConfigCustomization>,
    ): List<ConfigCustomization> = baseCustomizations + AwsChunkedConfigCustomization(codegenContext)

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ) = baseCustomizations + AwsChunkedOperationCustomization(codegenContext, operation)
}

private class AwsChunkedConfigCustomization(
    codegenContext: ClientCodegenContext,
) : ConfigCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: ServiceConfig) =
        writable {
            when (section) {
                ServiceConfig.BuilderImpl -> {
                    rustTemplate(
                        """
                        /// Sets the chunk size for [`aws-chunked encoding`].
                        ///
                        /// Pass `Some(size)` to use a specific chunk size (minimum 8 KiB).
                        /// Pass `None` to use the content-length as chunk size (no chunking).
                        ///
                        /// The minimum chunk size of 8 KiB is validated when the request is sent.
                        ///
                        /// **Note:** This setting only applies to operations that support aws-chunked encoding
                        /// and has no effect on other operations. When not set, a default chunk size of 64 KiB
                        /// is used.
                        ///
                        /// [`aws-chunked encoding`]: https://docs.aws.amazon.com/AmazonS3/latest/API/sigv4-streaming.html
                        ///
                        /// ## Example - Custom chunk size
                        /// ```no_run
                        /// ## use aws_sdk_s3::{Client, Config};
                        /// ## async fn example(client: Client) -> Result<(), Box<dyn std::error::Error>> {
                        /// let config = Config::builder()
                        ///     .chunk_size(Some(10240)) // 10 KiB chunks
                        ///     .build();
                        /// let client = Client::from_conf(config);
                        /// ## Ok(())
                        /// ## }
                        /// ```
                        ///
                        /// ## Example - No chunking
                        /// ```no_run
                        /// ## use aws_sdk_s3::{Client, Config};
                        /// ## async fn example(client: Client) -> Result<(), Box<dyn std::error::Error>> {
                        /// let config = Config::builder()
                        ///     .chunk_size(None) // Use entire content as one chunk
                        ///     .build();
                        /// let client = Client::from_conf(config);
                        /// ## Ok(())
                        /// ## }
                        /// ```
                        pub fn chunk_size(mut self, chunk_size: #{Option}<usize>) -> Self {
                            self.set_chunk_size(#{Some}(chunk_size));
                            self
                        }

                        /// Sets the chunk size for aws-chunked encoding.
                        pub fn set_chunk_size(&mut self, chunk_size: #{Option}<#{Option}<usize>>) -> &mut Self {
                            if let #{Some}(chunk_size) = chunk_size {
                                let chunk_size = match chunk_size {
                                    #{Some}(size) => #{ChunkSize}::Configured(size),
                                    #{None} => #{ChunkSize}::DisableChunking,
                                };
                                self.push_runtime_plugin(#{ChunkSizeRuntimePlugin}::new(chunk_size).into_shared());
                            }
                            self
                        }
                        """,
                        *preludeScope,
                        "ChunkSize" to runtimeConfig.awsChunked().resolve("ChunkSize"),
                        "ChunkSizeRuntimePlugin" to runtimeConfig.awsChunked().resolve("ChunkSizeRuntimePlugin"),
                    )
                }

                else -> emptySection
            }
        }
}

private class AwsChunkedOperationCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: OperationSection) =
        writable {
            // TODO(https://github.com/smithy-lang/smithy-rs/issues/4382): Remove all of these early returns
            //  once we have the dedicated trait available in Smithy.
            val checksumTrait = operation.getTrait<HttpChecksumTrait>() ?: return@writable
            val requestAlgorithmMember =
                checksumTrait.requestAlgorithmMemberShape(codegenContext, operation) ?: return@writable
            requestAlgorithmMember.getTrait<HttpHeaderTrait>()?.value ?: return@writable
            val input = model.expectShape(operation.inputShape, StructureShape::class.java)
            if (!input.hasStreamingMember(model)) {
                return@writable
            }

            when (section) {
                is OperationSection.AdditionalInterceptors -> {
                    section.registerInterceptor(runtimeConfig, this) {
                        rustTemplate(
                            """
                            #{AwsChunkedContentEncodingInterceptor}
                            """,
                            "AwsChunkedContentEncodingInterceptor" to
                                runtimeConfig.awsChunked()
                                    .resolve("AwsChunkedContentEncodingInterceptor"),
                        )
                    }
                }

                else -> emptySection
            }
        }
}

private fun RuntimeConfig.awsChunked() =
    RuntimeType.forInlineDependency(
        InlineAwsDependency.forRustFile(
            "aws_chunked", visibility = Visibility.PUBCRATE,
            CargoDependency.Bytes,
            CargoDependency.Http1x,
            CargoDependency.HttpBody1x,
            CargoDependency.Tracing,
            AwsCargoDependency.awsRuntime(this).withFeature("http-02x"),
            CargoDependency.smithyRuntimeApiClient(this),
            CargoDependency.smithyTypes(this),
            AwsCargoDependency.awsSigv4(this),
            CargoDependency.TempFile.toDevDependency(),
        ),
    )
