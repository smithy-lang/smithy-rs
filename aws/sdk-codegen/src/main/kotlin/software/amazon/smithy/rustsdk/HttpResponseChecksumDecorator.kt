/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull

private fun HttpChecksumTrait.requestValidationModeMember(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): MemberShape? {
    val requestValidationModeMember = this.requestValidationModeMember.orNull() ?: return null
    return operationShape.inputShape(codegenContext.model).expectMember(requestValidationModeMember)
}

class HttpResponseChecksumDecorator : ClientCodegenDecorator {
    override val name: String = "HttpResponseChecksum"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + HttpResponseChecksumCustomization(codegenContext, operation)
    }
}

// This generator was implemented based on this spec:
// https://awslabs.github.io/smithy/1.0/spec/aws/aws-core.html#http-request-checksums
class HttpResponseChecksumCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable {
        val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return emptySection
        val requestValidationModeMember =
            checksumTrait.requestValidationModeMember(codegenContext, operationShape) ?: return emptySection
        val requestValidationModeMemberInner = if (requestValidationModeMember.isOptional) {
            codegenContext.model.expectShape(requestValidationModeMember.target)
        } else {
            requestValidationModeMember
        }
        val validationModeName = codegenContext.symbolProvider.toMemberName(requestValidationModeMember)

        when (section) {
            is OperationSection.MutateRequest -> {
                // Otherwise, we need to set a property that the `MutateOutput` section handler will read to know if it
                // should checksum validate the response.
                return {
                    rustTemplate(
                        """
                        if let Some($validationModeName) = self.$validationModeName.as_ref() {
                            let $validationModeName = $validationModeName.clone();
                            // Place #{ValidationModeShape} in the property bag so we can check
                            // it during response deserialization to see if we need to checksum validate
                            // the response body.
                            let _ = request.properties_mut().insert($validationModeName);
                        }
                        """,
                        "ValidationModeShape" to codegenContext.symbolProvider.toSymbol(requestValidationModeMemberInner),
                    )
                }
            }
            is OperationSection.MutateOutput -> {
                // CRC32, CRC32C, SHA256, SHA1 -> "crc32", "crc32c", "sha256", "sha1"
                val responseAlgorithms = checksumTrait.responseAlgorithms
                    .map { algorithm -> algorithm.lowercase() }.joinToString(", ") { algorithm -> "\"$algorithm\"" }

                return {
                    rustTemplate(
                        """
                        let response_algorithms = [$responseAlgorithms].as_slice();
                        let $validationModeName = properties.get::<#{ValidationModeShape}>();
                        // Per [the spec](https://awslabs.github.io/smithy/1.0/spec/aws/aws-core.html##http-response-checksums),
                        // we check to see if it's the `ENABLED` variant
                        if matches!($validationModeName, Some(&#{ValidationModeShape}::Enabled)) {
                            if let Some((checksum_algorithm, precalculated_checksum)) =
                                #{check_headers_for_precalculated_checksum}(
                                    response.headers(),
                                    response_algorithms,
                                )
                            {
                                let bytestream = output.body.take().map(|bytestream| {
                                    bytestream.map(move |sdk_body| {
                                        #{wrap_body_with_checksum_validator}(
                                            sdk_body,
                                            checksum_algorithm,
                                            precalculated_checksum.clone(),
                                        )
                                    })
                                });
                                output = output.set_body(bytestream);
                            }
                        }
                        """,
                        "ValidationModeShape" to codegenContext.symbolProvider.toSymbol(requestValidationModeMemberInner),
                        "wrap_body_with_checksum_validator" to codegenContext.runtimeConfig.awsInlineableBodyWithChecksum().resolve("wrap_body_with_checksum_validator"),
                        "check_headers_for_precalculated_checksum" to codegenContext.runtimeConfig.awsInlineableBodyWithChecksum().resolve("check_headers_for_precalculated_checksum"),
                    )
                }
            }
            else -> return emptySection
        }
    }
}
