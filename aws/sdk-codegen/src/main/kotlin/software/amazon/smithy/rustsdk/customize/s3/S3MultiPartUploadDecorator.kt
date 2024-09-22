/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
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
            }
        }
}
