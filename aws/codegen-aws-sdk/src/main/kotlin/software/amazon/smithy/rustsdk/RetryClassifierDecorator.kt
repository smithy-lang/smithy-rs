/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.model.knowledge.OperationIndex
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable

class RetryClassifierDecorator : ClientCodegenDecorator {
    override val name: String = "RetryPolicy"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> =
        baseCustomizations +
            OperationRetryClassifiersFeature(codegenContext, operation)
}

class OperationRetryClassifiersFeature(
    private val codegenContext: ClientCodegenContext,
    val operation: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider

    override fun section(section: OperationSection) =
        when (section) {
            is OperationSection.RetryClassifiers ->
                writable {
                    section.registerRetryClassifier(this) {
                        val invalidCredentialErrors = invalidCredentialErrorsOverride(codegenContext, operation)
                        if (invalidCredentialErrors == null) {
                            rustTemplate(
                                "#{AwsErrorCodeClassifier}::<#{OperationError}>::new()",
                                "AwsErrorCodeClassifier" to AwsRuntimeType.awsRuntime(runtimeConfig).resolve("retries::classifiers::AwsErrorCodeClassifier"),
                                "OperationError" to symbolProvider.symbolForOperationError(operation),
                            )
                        } else {
                            rustTemplate(
                                "#{AwsErrorCodeClassifier}::<#{OperationError}>::builder()#{invalid_credential_errors}.build()",
                                "AwsErrorCodeClassifier" to AwsRuntimeType.awsRuntime(runtimeConfig).resolve("retries::classifiers::AwsErrorCodeClassifier"),
                                "OperationError" to symbolProvider.symbolForOperationError(operation),
                                "invalid_credential_errors" to invalidCredentialErrors,
                            )
                        }
                    }
                }

            else -> emptySection
        }
}

// Mirrors `INVALID_CREDENTIAL_ERRORS` in `aws-runtime/src/retries/classifiers.rs`. Keep the two
// lists in sync.
private val INVALID_CREDENTIAL_ERRORS = listOf("ExpiredToken", "ExpiredTokenException")

/**
 * Returns a `.invalid_credential_errors(..)` builder call for [operation], or null when the
 * operation models none of the invalid credential error codes.
 *
 * A code that the operation models describes a failure unrelated to the signing credentials, such
 * as an expired token in the request input. New credentials cannot fix that failure, so the code
 * is dropped from the list. The operation's errors include errors declared on the service. Each
 * error's code is the wire code that the protocol's deserializer matches.
 */
fun invalidCredentialErrorsOverride(
    codegenContext: ClientCodegenContext,
    operation: OperationShape,
): Writable? {
    val errorCode = { error: StructureShape ->
        codegenContext.protocolImpl?.httpBindingResolver?.errorCode(error) ?: error.id.name
    }
    val modeledCodes =
        OperationIndex.of(codegenContext.model)
            .getErrors(operation, codegenContext.serviceShape)
            .map(errorCode)
            .toSet()
    val remaining = INVALID_CREDENTIAL_ERRORS.filterNot { it in modeledCodes }
    if (remaining.size == INVALID_CREDENTIAL_ERRORS.size) {
        return null
    }
    val codes = remaining.joinToString(", ") { "\"$it\"" }
    return writable { rust(".invalid_credential_errors(&[$codes] as &[&'static str])") }
}
