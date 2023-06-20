/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rustsdk

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.shapes.ShapeId
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.customize.ClientCodegenDecorator
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Visibility
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeConfig
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.letIf
import software.amazon.smithy.rust.codegen.core.util.orNull

private fun RuntimeConfig.awsInlineableHttpResponseChecksum() = RuntimeType.forInlineDependency(
    InlineAwsDependency.forRustFile(
        "http_response_checksum", visibility = Visibility.PUBCRATE,
        CargoDependency.Bytes,
        CargoDependency.Http,
        CargoDependency.HttpBody,
        CargoDependency.Tracing,
        CargoDependency.smithyChecksums(this),
        CargoDependency.smithyHttp(this),
        CargoDependency.smithyRuntimeApi(this),
        CargoDependency.smithyTypes(this),
    ),
)

fun HttpChecksumTrait.requestValidationModeMember(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): MemberShape? {
    val requestValidationModeMember = this.requestValidationModeMember.orNull() ?: return null
    return operationShape.inputShape(codegenContext.model).expectMember(requestValidationModeMember)
}

class HttpResponseChecksumDecorator : ClientCodegenDecorator {
    override val name: String = "HttpResponseChecksum"
    override val order: Byte = 0

    private fun applies(operationShape: OperationShape): Boolean =
        operationShape.outputShape != ShapeId.from("com.amazonaws.s3#GetObjectOutput")

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> = baseCustomizations.letIf(applies(operation)) {
        it + HttpResponseChecksumCustomization(codegenContext, operation)
    }
}

// This generator was implemented based on this spec:
// https://awslabs.github.io/smithy/1.0/spec/aws/aws-core.html#http-request-checksums
class HttpResponseChecksumCustomization(
    private val codegenContext: ClientCodegenContext,
    private val operationShape: OperationShape,
) : OperationCustomization() {
    override fun section(section: OperationSection): Writable = writable {
        val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return@writable
        val requestValidationModeMember =
            checksumTrait.requestValidationModeMember(codegenContext, operationShape) ?: return@writable
        val requestValidationModeMemberInner = if (requestValidationModeMember.isOptional) {
            codegenContext.model.expectShape(requestValidationModeMember.target)
        } else {
            requestValidationModeMember
        }
        val validationModeName = codegenContext.symbolProvider.toMemberName(requestValidationModeMember)
        val inputShape = codegenContext.model.expectShape(operationShape.inputShape)

        when (section) {
            is OperationSection.AdditionalInterceptors -> {
                section.registerInterceptor(codegenContext.runtimeConfig, this) {
                    // CRC32, CRC32C, SHA256, SHA1 -> "crc32", "crc32c", "sha256", "sha1"
                    val responseAlgorithms = checksumTrait.responseAlgorithms
                        .map { algorithm -> algorithm.lowercase() }.joinToString(", ") { algorithm -> "\"$algorithm\"" }
                    val runtimeApi = RuntimeType.smithyRuntimeApi(codegenContext.runtimeConfig)
                    rustTemplate(
                        """
                        #{ResponseChecksumInterceptor}::new(
                            [$responseAlgorithms].as_slice(),
                            |input: &#{Input}| {
                                ${""/*
                                Per [the spec](https://smithy.io/2.0/aws/aws-core.html#http-response-checksums),
                                we check to see if it's the `ENABLED` variant
                                */}
                                let input: &#{OperationInput} = input.downcast_ref().expect("correct type");
                                matches!(input.$validationModeName(), #{Some}(#{ValidationModeShape}::Enabled))
                            }
                        )
                        """,
                        *preludeScope,
                        "ResponseChecksumInterceptor" to codegenContext.runtimeConfig.awsInlineableHttpResponseChecksum()
                            .resolve("ResponseChecksumInterceptor"),
                        "Input" to runtimeApi.resolve("client::interceptors::context::Input"),
                        "OperationInput" to codegenContext.symbolProvider.toSymbol(inputShape),
                        "ValidationModeShape" to codegenContext.symbolProvider.toSymbol(requestValidationModeMemberInner),
                    )
                }
            }
            // TODO(enableNewSmithyRuntimeCleanup): Delete `is OperationSection.MutateRequest`
            is OperationSection.MutateRequest -> {
                // Otherwise, we need to set a property that the `MutateOutput` section handler will read to know if it
                // should checksum validate the response.
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
            // TODO(enableNewSmithyRuntimeCleanup): Delete `is OperationSection.MutateOutput`
            is OperationSection.MutateOutput -> {
                if (!section.propertyBagAvailable) {
                    return@writable
                }

                // CRC32, CRC32C, SHA256, SHA1 -> "crc32", "crc32c", "sha256", "sha1"
                val responseAlgorithms = checksumTrait.responseAlgorithms
                    .map { algorithm -> algorithm.lowercase() }.joinToString(", ") { algorithm -> "\"$algorithm\"" }

                rustTemplate(
                    """
                    let response_algorithms = [$responseAlgorithms].as_slice();
                    let $validationModeName = properties.get::<#{ValidationModeShape}>();
                    // Per [the spec](https://smithy.io/2.0/aws/aws-core.html##http-response-checksums),
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
                    "wrap_body_with_checksum_validator" to codegenContext.runtimeConfig.awsInlineableBodyWithChecksumMiddleware()
                        .resolve("wrap_body_with_checksum_validator"),
                    "check_headers_for_precalculated_checksum" to codegenContext.runtimeConfig.awsInlineableBodyWithChecksumMiddleware()
                        .resolve("check_headers_for_precalculated_checksum"),
                )
            }

            else -> {}
        }
    }
}
