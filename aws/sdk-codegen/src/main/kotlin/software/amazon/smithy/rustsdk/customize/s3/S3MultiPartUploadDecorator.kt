/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.node.Node
import software.amazon.smithy.model.shapes.EnumShape
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.ServiceShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.model.traits.DefaultTrait
import software.amazon.smithy.model.transform.ModelTransformer
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustSettings
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.util.letIf
import java.util.logging.Logger

/**
 * Add a default value to CreateMultiPartUploadRequest's ChecksumAlgorithm
 */
class S3MultiPartUploadDecorator : ClientCodegenDecorator {
    override val name: String = "S3MultiPartUpload"
    override val order: Byte = 0
    private val logger: Logger = Logger.getLogger(javaClass.name)
    private val defaultAlgorithm = "CRC32"
    private val targetEnumName = "ChecksumAlgorithm"
    private val requestName = "CreateMultipartUploadRequest"
    private val s3Namespace = "com.amazonaws.s3"

    private fun isChecksumAlgorithm(shape: Shape): Boolean =
        shape is EnumShape && shape.id == ShapeId.from("$s3Namespace#$targetEnumName")

    private fun isChecksumAlgoRequestMember(shape: Shape): Boolean =
        shape is MemberShape && shape.id == ShapeId.from("$s3Namespace#$requestName$$targetEnumName")

    override fun transformModel(
        service: ServiceShape,
        model: Model,
        settings: ClientRustSettings,
    ): Model =
        ModelTransformer.create().mapShapes(model) { shape ->
            shape.letIf(isChecksumAlgorithm(shape)) {
                // Update root checksum algo enum with a default
                (shape as EnumShape).toBuilder().addTrait(DefaultTrait(Node.from(defaultAlgorithm))).build()
            }.letIf(isChecksumAlgoRequestMember(shape)) {
                // Update the default on the CreateMPURequest shape
                (shape as MemberShape).toBuilder().addTrait(DefaultTrait(Node.from(defaultAlgorithm))).build()
            }
        }
}
