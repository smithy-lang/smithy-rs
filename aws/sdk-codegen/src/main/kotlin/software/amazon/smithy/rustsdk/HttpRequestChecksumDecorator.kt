/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.asType
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.smithy.customize.RustCodegenDecorator
import software.amazon.smithy.rust.codegen.smithy.generators.operationBuildError
import software.amazon.smithy.rust.codegen.util.expectMember
import software.amazon.smithy.rust.codegen.util.getTrait
import software.amazon.smithy.rust.codegen.util.hasStreamingMember
import software.amazon.smithy.rust.codegen.util.inputShape
import software.amazon.smithy.rust.codegen.util.orNull

fun RuntimeConfig.awsInlineableBodyWithChecksum() = RuntimeType.forInlineDependency(
    InlineAwsDependency.forRustFile(
        "body_with_checksum", visibility = Visibility.PUBLIC,
        CargoDependency.Http,
        CargoDependency.HttpBody,
        CargoDependency.SmithyHttp(this),
        CargoDependency.SmithyChecksums(this),
        CargoDependency.SmithyTypes(this),
        CargoDependency.Bytes,
        CargoDependency.Tracing,
        this.awsRuntimeDependency("aws-http")
    )
)

class HttpRequestChecksumDecorator : RustCodegenDecorator<ClientCodegenContext> {
    override val name: String = "HttpRequestChecksum"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>
    ): List<OperationCustomization> {
        return baseCustomizations + HttpRequestChecksumCustomization(codegenContext, operation)
    }
}

private fun HttpChecksumTrait.requestAlgorithmMember(codegenContext: ClientCodegenContext, operationShape: OperationShape): String? {
    val requestAlgorithmMember = this.requestAlgorithmMember.orNull() ?: return null
    val checksumAlgorithmMemberShape = operationShape.inputShape(codegenContext.model).expectMember(requestAlgorithmMember)
    return codegenContext.symbolProvider.toMemberName(checksumAlgorithmMemberShape)
}

private fun HttpChecksumTrait.checksumAlgorithmToStr(codegenContext: ClientCodegenContext, operationShape: OperationShape): Writable {
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

        // If a request checksum is not required and there's no way to set one, do nothing
        // This happens when an operation only supports response checksums
    }
}

// This generator was implemented based on this spec:
// https://awslabs.github.io/smithy/1.0/spec/aws/aws-core.html#http-request-checksums
class HttpRequestChecksumCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape
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
                }
                if (operationShape.inputShape(codegenContext.model).hasStreamingMember(codegenContext.model)) {
                    return {
                        rustTemplate(
                            """
                            ${section.request} = ${section.request}.augment(|mut req, properties| {
                                #{checksum_algorithm_to_str:W}
                                if let Some(checksum_algorithm) = checksum_algorithm {
                                    properties.insert(#{sig_auth}::signer::SignableBody::StreamingUnsignedPayloadTrailer);
                                    #{build_checksum_validated_request_with_streaming_body}(&mut req, checksum_algorithm)?;
                                }

                                Result::<_, #{BuildError}>::Ok(req)
                            })?;
                            """,
                            "sig_auth" to runtimeConfig.awsRuntimeDependency("aws-sig-auth").asType(),
                            "build_checksum_validated_request_with_streaming_body" to runtimeConfig.awsInlineableBodyWithChecksum()
                                .member("build_checksum_validated_request_with_streaming_body"),
                            "BuildError" to runtimeConfig.operationBuildError(),
                            "checksum_algorithm_to_str" to checksumTrait.checksumAlgorithmToStr(codegenContext, operationShape),
                        )
                    }
                } else {
                    return {
                        rustTemplate(
                            """
                            ${section.request} = ${section.request}.augment(|mut req, _| {
                                #{checksum_algorithm_to_str:W}
                                if let Some(checksum_algorithm) = checksum_algorithm {
                                    #{build_checksum_validated_request}(&mut req, checksum_algorithm)?;
                                }

                                Result::<_, #{BuildError}>::Ok(req)
                            })?;
                            """,
                            "build_checksum_validated_request" to runtimeConfig.awsInlineableBodyWithChecksum()
                                .member("build_checksum_validated_request"),
                            "BuildError" to runtimeConfig.operationBuildError(),
                            "checksum_algorithm_to_str" to checksumTrait.checksumAlgorithmToStr(codegenContext, operationShape),
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
