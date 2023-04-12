/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull

fun RuntimeConfig.awsInlineableBodyWithChecksum() = RuntimeType.forInlineDependency(
    InlineAwsDependency.forRustFile(
        "http_body_checksum", visibility = Visibility.PUBLIC,
        CargoDependency.Http,
        CargoDependency.HttpBody,
        CargoDependency.smithyHttp(this),
        CargoDependency.smithyChecksums(this),
        CargoDependency.smithyTypes(this),
        CargoDependency.Bytes,
        CargoDependency.Tracing,
    ),
)

class HttpRequestChecksumDecorator : ClientCodegenDecorator {
    override val name: String = "HttpRequestChecksum"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        return baseCustomizations + HttpRequestChecksumCustomization(codegenContext, operation)
    }
}

private fun HttpChecksumTrait.requestAlgorithmMember(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): String? {
    val requestAlgorithmMember = this.requestAlgorithmMember.orNull() ?: return null
    val checksumAlgorithmMemberShape =
        operationShape.inputShape(codegenContext.model).expectMember(requestAlgorithmMember)

    return codegenContext.symbolProvider.toMemberName(checksumAlgorithmMemberShape)
}

private fun HttpChecksumTrait.checksumAlgorithmToStr(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): Writable {
    val runtimeConfig = codegenContext.runtimeConfig
    val requestAlgorithmMember = this.requestAlgorithmMember(codegenContext, operationShape)
    val isRequestChecksumRequired = this.isRequestChecksumRequired

    return {
        if (requestAlgorithmMember != null) {
            // User may set checksum for requests, and we need to call as_ref before we can convert the algorithm to a &str
            rust("let checksum_algorithm = $requestAlgorithmMember.as_ref();")

            if (isRequestChecksumRequired) {
                // Checksums are required, fall back to MD5
                rust("""let checksum_algorithm = checksum_algorithm.map(|algorithm| algorithm.as_str()).or(Some("md5"));""")
            } else {
                // Checksums aren't required, don't set a fallback
                rust("let checksum_algorithm = checksum_algorithm.map(|algorithm| algorithm.as_str());")
            }
        } else if (isRequestChecksumRequired) {
            // Checksums are required but a user can't set one, so we set MD5 for them
            rust("""let checksum_algorithm = Some("md5");""")
        }

        rustTemplate(
            """
            let checksum_algorithm = match checksum_algorithm {
                Some(algo) => Some(
                    algo.parse::<#{ChecksumAlgorithm}>()
                    .map_err(#{BuildError}::other)?
                ),
                None => None,
            };
            """,
            "BuildError" to runtimeConfig.operationBuildError(),
            "ChecksumAlgorithm" to RuntimeType.smithyChecksums(runtimeConfig).resolve("ChecksumAlgorithm"),
        )

        // If a request checksum is not required and there's no way to set one, do nothing
        // This happens when an operation only supports response checksums
    }
}

// This generator was implemented based on this spec:
// https://awslabs.github.io/smithy/1.0/spec/aws/aws-core.html#http-request-checksums
class HttpRequestChecksumCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    private val runtimeConfig = codegenContext.runtimeConfig

    override fun section(section: OperationSection): Writable {
        // Get the `HttpChecksumTrait`, returning early if this `OperationShape` doesn't have one
        val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return emptySection
        val checksumAlgorithm = checksumTrait.requestAlgorithmMember(codegenContext, operationShape)

        return when (section) {
            is OperationSection.MutateInput -> {
                // Various other things will consume the input struct before we can get at the checksum algorithm
                // field within it. This ensures that we preserve a copy of it. It's an enum so cloning is cheap.
                if (checksumAlgorithm != null) {
                    return {
                        rust("let $checksumAlgorithm = self.$checksumAlgorithm().cloned();")
                    }
                } else {
                    emptySection
                }
            }
            is OperationSection.MutateRequest -> {
                // Return early if no request checksum can be set nor is it required
                if (!checksumTrait.isRequestChecksumRequired && checksumAlgorithm == null) {
                    return emptySection
                } else {
                    // `add_checksum_calculation_to_request` handles both streaming and in-memory request bodies.
                    return {
                        rustTemplate(
                            """
                            ${section.request} = ${section.request}.augment(|mut req, properties| {
                                #{checksum_algorithm_to_str:W}
                                if let Some(checksum_algorithm) = checksum_algorithm {
                                    #{add_checksum_calculation_to_request}(&mut req, properties, checksum_algorithm)?;
                                }
                                Result::<_, #{BuildError}>::Ok(req)
                            })?;
                            """,
                            "checksum_algorithm_to_str" to checksumTrait.checksumAlgorithmToStr(
                                codegenContext,
                                operationShape,
                            ),
                            "add_checksum_calculation_to_request" to runtimeConfig.awsInlineableBodyWithChecksum()
                                .resolve("add_checksum_calculation_to_request"),
                            "BuildError" to runtimeConfig.operationBuildError(),
                        )
                    }
                }
            }
            else -> {
                return emptySection
            }
        }
    }
}
