/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.smithy.generators.ProtocolConfig

class RetryPolicyDecorator : RustCodegenDecorator {
    override val name: String = "RetryPolicy"
    override val order: Byte = 0

    override fun operationCustomizations(
        protocolConfig: ProtocolConfig,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + RetryPolicyFeature(protocolConfig.runtimeConfig)
    }
}

class RetryPolicyFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun retryType(): RuntimeType = runtimeConfig.awsHttp().asType().copy(name = "AwsErrorRetryPolicy")
    override fun section(section: OperationSection) = when (section) {
        is OperationSection.FinalizeOperation -> writable {
            rust(
                "let ${section.operation} = ${section.operation}.with_retry_policy(#T::AwsErrorRetryPolicy::new());",
                runtimeConfig.awsHttp().asType()
            )
        }
        else -> emptySection
    }
}
