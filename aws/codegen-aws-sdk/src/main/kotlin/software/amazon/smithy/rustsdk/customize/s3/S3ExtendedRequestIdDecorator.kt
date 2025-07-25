/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rustsdk.BaseRequestIdDecorator
import software.amazon.smithy.rustsdk.InlineAwsDependency

class S3ExtendedRequestIdDecorator : BaseRequestIdDecorator() {
    override val name: String = "S3ExtendedRequestIdDecorator"
    override val order: Byte = 0

    override val fieldName: String = "extended_request_id"
    override val accessorFunctionName: String = "extended_request_id"

    override fun asMemberShape(container: StructureShape): MemberShape? {
        return null
    }

    private val requestIdModule: RuntimeType =
        RuntimeType.forInlineDependency(InlineAwsDependency.forRustFile("s3_request_id"))

    override fun accessorTrait(codegenContext: ClientCodegenContext): RuntimeType =
        requestIdModule.resolve("RequestIdExt")

    override fun applyToError(codegenContext: ClientCodegenContext): RuntimeType =
        requestIdModule.resolve("apply_extended_request_id")
}
