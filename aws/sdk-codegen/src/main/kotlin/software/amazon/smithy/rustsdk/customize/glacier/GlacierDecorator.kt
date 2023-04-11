/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization

val Glacier: ShapeId = ShapeId.from("com.amazonaws.glacier#Glacier")

class GlacierDecorator : ClientCodegenDecorator {
    override val name: String = "Glacier"
    override val order: Byte = 0

    private fun applies(codegenContext: CodegenContext) = codegenContext.serviceShape.id == Glacier

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        val extras = if (applies(codegenContext)) {
            val apiVersion = codegenContext.serviceShape.version
            listOfNotNull(
                ApiVersionHeader(apiVersion),
                TreeHashHeader.forOperation(operation, codegenContext.runtimeConfig),
                AccountIdAutofill.forOperation(operation, codegenContext.model),
            )
        } else {
            emptyList()
        }
        return baseCustomizations + extras
    }
}
