/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.rustlang.asType
import software.amazon.smithy.rust.codegen.client.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.client.smithy.customize.RustCodegenDecorator

class RetryClassifierDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "RetryPolicy"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + RetryClassifierFeature(codegenContext.runtimeConfig)
    }

    override fun supportsCodegenContext(clazz: Class<out CoreCodegenContext>): Boolean =
        clazz.isAssignableFrom(ClientCodegenContext::class.java)
}

class RetryClassifierFeature(private val runtimeConfig: RuntimeConfig) : OperationCustomization() {
    override fun retryType(): RuntimeType = runtimeConfig.awsHttp().asType().member("retry::AwsResponseRetryClassifier")
    override fun section(section: OperationSection) = when (section) {
        is OperationSection.FinalizeOperation -> writable {
            rust(
                "let ${section.operation} = ${section.operation}.with_retry_classifier(#T::new());",
                retryType(),
            )
        }
        else -> emptySection
    }
}
