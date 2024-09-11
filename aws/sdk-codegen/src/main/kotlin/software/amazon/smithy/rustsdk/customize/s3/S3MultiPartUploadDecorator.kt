/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * Add a default value to CreateMultiPartUploadRequest's ChecksumAlgorithm
 */
class S3MultiPartUploadDecorator : ClientCodegenDecorator {
    override val name: String = "S3MultiPartUpload"
    override val order: Byte = 0
    private val defaultAlgorithm = "CRC32"

    private val operationName = "CreateMultipartUpload"
    private val s3Namespace = "com.amazonaws.s3"

    private fun isS3MPUOperation(shape: Shape): Boolean =
        shape is OperationShape && shape.id == ShapeId.from("$s3Namespace#$operationName")

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        if (isS3MPUOperation(operation)) {
            baseCustomizations + listOf(S3MPUChecksumCustomization(codegenContext, operation))
        } else {
            baseCustomizations
        }
}

/**
 * S3 CreateMPU is not modeled with the httpChecksum trait so the checksum_algorithm
 * value must be defaulted here in an interceptor.
 */
private class S3MPUChecksumCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable =
        writable {
            val targetEnumName = "ChecksumAlgorithm"
            val interceptorName = "S3CreateMultipartUploadInterceptor"
            val rc = codegenContext.runtimeConfig
            val checksumAlgoShape =
                operation.inputShape(codegenContext.model).getMember(targetEnumName).orNull() ?: return@writable
            val checksumAlgoInner = codegenContext.model.expectShape(checksumAlgoShape.target)
            if (section is OperationSection.AdditionalInterceptors) {
                section.registerInterceptor(codegenContext.runtimeConfig, this) {
                    rust(interceptorName)
                }
            }
            if (section is OperationSection.RuntimePluginSupportingTypes) {
//                section.defineType(this) {
                rustTemplate(
                    """
                    ##[derive(Debug)]
                    struct $interceptorName;

                    impl #{Intercept}
                        for $interceptorName
                    {
                        fn name(&self) -> &'static str {
                            "$interceptorName"
                        }

                        fn modify_before_serialization(
                            &self,
                            context: &mut #{Context},
                            _runtime_components: &#{RuntimeComponents},
                            _cfg: &mut #{ConfigBag},
                        ) -> Result<(), #{BoxError}> {
                            let input = context
                                .input_mut()
                                .downcast_mut::<#{OperationInputType}>()
                                .expect("Input should be for S3 CreateMultipartUpload");

                            // S3 CreateMPU is not modeled with the httpChecksum trait so the checksum_algorithm
                            // value must be defaulted here.
                            if input.checksum_algorithm.is_none() {
                                input.checksum_algorithm = Some(#{ChecksumAlgo}::Crc32)
                            }

                            Ok(())
                        }
                    }
                    """.trimIndent(),
                    "BoxError" to RuntimeType.boxError(rc),
                    "ConfigBag" to RuntimeType.configBag(rc),
                    "RuntimeComponents" to RuntimeType.runtimeComponents(rc),
                    "Context" to RuntimeType.beforeSerializationInterceptorContextMut(rc),
                    "Intercept" to RuntimeType.intercept(rc),
                    "OperationInputType" to
                        codegenContext.symbolProvider.toSymbol(
                            operation.inputShape(
                                codegenContext.model,
                            ),
                        ),
                    "ChecksumAlgo" to codegenContext.symbolProvider.toSymbol(checksumAlgoInner),
                )
//                }
            }
        }
}
