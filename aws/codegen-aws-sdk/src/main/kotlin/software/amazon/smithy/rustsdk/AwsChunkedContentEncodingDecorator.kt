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
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.hasStreamingMember

class AwsChunkedContentEncodingDecorator : ClientCodegenDecorator {
    override val name: String = "AwsChunkedContentEncoding"

    // This decorator must decorate after any of the following:
    // - HttpRequestChecksumDecorator
    // - HttpRequestCompressionDecorator
    override val order: Byte = (minOf(HttpRequestChecksumDecorator.ORDER, HttpRequestCompressionDecorator.ORDER) - 1).toByte()

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ) = baseCustomizations + AwsChunkedOparationCustomization(codegenContext, operation)
}

private class AwsChunkedOparationCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: OperationSection) =
        writable {
            when (section) {
                is OperationSection.AdditionalInterceptors -> {
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
            CargoDependency.Http,
            CargoDependency.HttpBody,
            CargoDependency.Tracing,
            AwsCargoDependency.awsRuntime(this).withFeature("http-02x"),
            CargoDependency.smithyRuntimeApiClient(this),
            CargoDependency.smithyTypes(this),
            AwsCargoDependency.awsSigv4(this),
            CargoDependency.TempFile.toDevDependency(),
        ),
    )
