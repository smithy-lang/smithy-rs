/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk.customize.cloudwatch

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig
import software.amazon.smithy.rust.codegen.smithy.letIf

class CloudWatchDecorator : RustCodegenDecorator {
    override val name: String = "CloudWatch"
    override val order: Byte = 0

    private fun applies(protocolConfig: ProtocolConfig) = protocolConfig.serviceShape.id == ShapeId.from("com.amazonaws.cloudwatchlogs#PutLogEvents")
    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations.letIf(applies(protocolConfig)) {
            it + CloudWatchAddLogFormatHeader()
        }
    }
}

class CloudWatchAddLogFormatHeader : OperationCustomization() {
    override fun section(section: OperationSection): Writable = when (section) {
        is OperationSection.FinalizeOperation -> emptySection
        OperationSection.OperationImplBlock -> emptySection
        is OperationSection.MutateRequest -> writable {
            rust(
                """${section.request}
                .request_mut()
                .headers_mut()
                .insert("x-amzn-logs-format", #T::HeaderValue::from_static("json/emf"));""",
                RuntimeType.http
            )
        }
        else -> emptySection
    }
}