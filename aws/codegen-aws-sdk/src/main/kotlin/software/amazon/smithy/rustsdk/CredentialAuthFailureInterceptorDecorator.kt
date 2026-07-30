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
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable

/**
 * Registers `CredentialAuthFailureInterceptor` on every operation (F-INVAL-1 / D-INVMARK).
 *
 * When a target service rejects a request with an `ExpiredToken`/`InvalidToken` error, the
 * interceptor sets a data-free config-bag marker; the orchestrator consumes the marker and calls
 * `identity_cache().invalidate(&identity)` on the signing identity, so the next resolve refreshes
 * the rejected credentials rather than reusing them.
 *
 * The interceptor is generic over the operation's error type so it can downcast the type-erased
 * deserialized error and read its code. Like `AwsErrorCodeClassifier` (see [RetryClassifierDecorator]),
 * it is therefore registered *per operation* with that operation's error type.
 */
class CredentialAuthFailureInterceptorDecorator : ClientCodegenDecorator {
    override val name: String = "CredentialAuthFailureInterceptor"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations + CredentialAuthFailureInterceptorFeature(codegenContext, operation)
}

private class CredentialAuthFailureInterceptorFeature(
    codegenContext: ClientCodegenContext,
    private val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection) =
        when (section) {
            is OperationSection.AdditionalInterceptors ->
                writable {
                    section.registerInterceptor(runtimeConfig, this) {
                        rustTemplate(
                            "#{CredentialAuthFailureInterceptor}::<#{OperationError}>::new()",
                            "CredentialAuthFailureInterceptor" to
                                AwsRuntimeType.awsRuntime(runtimeConfig)
                                    .resolve("static_stability::invalidation::CredentialAuthFailureInterceptor"),
                            "OperationError" to symbolProvider.symbolForOperationError(operation),
                        )
                    }
                }

            else -> emptySection
        }
}
