/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.util.dq

/**
 * Adds the AWS SDK specific `rpc.system = "aws-api"` field to the operation's tracing span
 */
class SpanDecorator : ClientCodegenDecorator {
    override val name: String = "SpanDecorator"
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
                            is OperationSection.AdditionalOperationSpanFields -> {
                                section.addField(this, "rpc.system", writable("aws-api".dq()))
                            }

                            else -> {}
                        }
                    }
                }
            }
    }
}
