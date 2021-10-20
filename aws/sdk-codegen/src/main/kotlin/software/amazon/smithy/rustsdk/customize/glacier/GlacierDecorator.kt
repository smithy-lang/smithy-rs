/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk.customize.glacier

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator

class GlacierDecorator : RustCodegenDecorator {
    override val name: String = "Glacier"
    override val order: Byte = 0

    private fun applies(codegenContext: CodegenContext) = codegenContext.serviceShape.id == ShapeId.from("com.amazonaws.glacier#Glacier")
    override fun operationCustomizations(
        codegenContext: CodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        val extras = if (applies(codegenContext)) {
            val apiVersion = codegenContext.serviceShape.version
            listOf(ApiVersionHeader(apiVersion))
        } else {
            emptyList()
        }
        return baseCustomizations + extras
    }
}
