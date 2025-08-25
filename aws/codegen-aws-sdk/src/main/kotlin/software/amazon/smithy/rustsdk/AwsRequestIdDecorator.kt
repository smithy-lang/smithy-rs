/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType

/**
 * Customizes response parsing logic to add AWS request IDs to error metadata and outputs
 */
class AwsRequestIdDecorator : BaseRequestIdDecorator() {
    override val name: String = "AwsRequestIdDecorator"
    override val order: Byte = 0

    override val fieldName: String = "request_id"
    override val accessorFunctionName: String = "request_id"

    private fun requestIdModule(codegenContext: ClientCodegenContext): RuntimeType =
        AwsRuntimeType.awsTypes(codegenContext.runtimeConfig).resolve("request_id")

    override fun accessorTrait(codegenContext: ClientCodegenContext): RuntimeType =
        requestIdModule(codegenContext).resolve("RequestId")

    override fun applyToError(codegenContext: ClientCodegenContext): RuntimeType =
        requestIdModule(codegenContext).resolve("apply_request_id")
}
