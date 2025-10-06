/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope

internal val DISABLE_PAYLOAD_SIGNING_OPERATIONS by lazy {
    listOf(
        // S3
        ShapeId.from("com.amazonaws.s3#PutObject"),
        ShapeId.from("com.amazonaws.s3#UploadPart"),
    )
}

class DisablePayloadSigningDecorator : ClientCodegenDecorator {
    override val name: String = "DisablePayloadSigning"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations +
            object : OperationCustomization() {
                private val runtimeConfig = codegenContext.runtimeConfig

                override fun section(section: OperationSection): Writable {
                    return writable {
                        when (section) {
                            is OperationSection.CustomizableOperationImpl -> {
                                if (DISABLE_PAYLOAD_SIGNING_OPERATIONS.contains(operation.id)) {
                                    rustTemplate(
                                        """
                                        /// Disable payload signing for this request.
                                        ///
                                        /// **WARNING:** This is an advanced feature that removes
                                        /// the cost of signing a request payload by removing a data
                                        /// integrity check. Not all services/operations support
                                        /// this feature.
                                        pub fn disable_payload_signing(self) -> Self {
                                            self.runtime_plugin(#{PayloadSigningOverrideRuntimePlugin}::unsigned())
                                        }
                                        """,
                                        *preludeScope,
                                        "PayloadSigningOverrideRuntimePlugin" to
                                            AwsRuntimeType.awsRuntime(runtimeConfig)
                                                .resolve("auth::PayloadSigningOverrideRuntimePlugin"),
                                    )
                                }
                            }

                            else -> {}
                        }
                    }
                }
            }
    }
}
