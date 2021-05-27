/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk.customize.s3

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.LibRsSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.letIf
import software.amazon.smithy.rustsdk.AwsRuntimeType

class S3ExtendedErrorDecorator : RustCodegenDecorator {
    override val name: String = "S3ExtendedError"
    override val order: Byte = 0
    private fun applies(protocolConfig: ProtocolConfig) =
        protocolConfig.serviceShape.id == ShapeId.from("com.amazonaws.s3#AmazonS3")

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations.letIf(applies(protocolConfig)) {
            it + S3ParseExtendedErrors()
        }
    }

    override fun libRsCustomizations(
        protocolConfig: ProtocolConfig,
        baseCustomizations: List<LibRsCustomization>
    ): List<LibRsCustomization> {
        return baseCustomizations.letIf(applies(protocolConfig)) {
            it + S3PubUse()
        }
    }
}

class S3PubUse : LibRsCustomization() {
    override fun section(section: LibRsSection): Writable = when (section) {
        is LibRsSection.Body -> writable { rust("pub use #T::ErrorExt;", AwsRuntimeType.S3Errors) }
        else -> emptySection
    }
}

class S3ParseExtendedErrors : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.UpdateGenericError -> writable {
            rust(
                "let ${section.genericError} = #T::parse_extended_error(${section.genericError}, &${section.httpResponse});",
                AwsRuntimeType.S3Errors
            )
        }
        else -> emptySection
    }
}
