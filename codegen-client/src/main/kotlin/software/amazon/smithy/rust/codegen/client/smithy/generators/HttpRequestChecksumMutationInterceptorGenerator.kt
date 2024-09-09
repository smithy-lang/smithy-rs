/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.generators

import software.amazon.smithy.aws.traits.HttpChecksumTrait
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.rust.codegen.client.smithy.ClientCodegenContext
import software.amazon.smithy.rust.codegen.client.smithy.endpoint.EndpointTypesGenerator
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * This class generates an interceptor for operations with the `httpChecksum` trait that support request checksums.
 * In the `modify_before_serialization` hook the interceptor checks the config's `request_checksum_calculation` value
 * and the trait's `requestChecksumRequired` value. If `request_checksum_calculation` is `WhenSupported` or it is
 * `WhenRequired` and `requestChecksumRequired` is `true` then we check the operation input's  `requestAlgorithmMember`.
 * If that is `None` (so has not been explicitly set by the user) we default it to `Crc32`.
 *
 * Note that although there is an existing inlineable `RequestChecksumInterceptor` this logic could not live there.
 * Since that interceptor is inlineable it does not have access to the name of the `requestAlgorithmMember` on the
 * operation's input or the `requestChecksumRequired` value from the trait, and in certain circumstances we need to
 * mutate that member on the input before serializing the request and sending it to the service.
 */
class HttpRequestChecksumMutationInterceptorGenerator(
    private val codegenContext: ClientCodegenContext,
) {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val codegenScope =
        codegenContext.runtimeConfig.let { rc ->
            val endpointTypesGenerator = EndpointTypesGenerator.fromContext(codegenContext)
            val runtimeApi = CargoDependency.smithyRuntimeApiClient(rc).toType()
            val interceptors = runtimeApi.resolve("client::interceptors")
            val orchestrator = runtimeApi.resolve("client::orchestrator")

            arrayOf(
                *preludeScope,
                "BoxError" to RuntimeType.boxError(rc),
                "ConfigBag" to RuntimeType.configBag(rc),
                "ContextAttachedError" to interceptors.resolve("error::ContextAttachedError"),
                "HttpRequest" to orchestrator.resolve("HttpRequest"),
                "HttpResponse" to orchestrator.resolve("HttpResponse"),
                "Intercept" to RuntimeType.intercept(rc),
                "InterceptorContext" to RuntimeType.interceptorContext(rc),
                "BeforeSerializationInterceptorContextMut" to RuntimeType.beforeSerializationInterceptorContextMut(rc),
                "Input" to interceptors.resolve("context::Input"),
                "Output" to interceptors.resolve("context::Output"),
                "Error" to interceptors.resolve("context::Error"),
                "InterceptorError" to interceptors.resolve("error::InterceptorError"),
                "Params" to endpointTypesGenerator.paramsStruct(),
                "RuntimeComponents" to RuntimeType.runtimeComponents(rc),
                "RequestChecksumCalculation" to
                    CargoDependency.smithyChecksums(rc).toType()
                        .resolve("RequestChecksumCalculation"),
            )
        }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        // If the operation doesn't have the HttpChecksum trait we return early
        val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return
        // Also return early if there is no requestValidationModeMember on the trait
        val requestAlgorithmMember =
            (checksumTrait.requestAlgorithmMember(codegenContext, operationShape) ?: return)
        val requestAlgorithmMemberName = symbolProvider.toSymbol(requestAlgorithmMember).name
        val requestAlgorithmMemberInner =
            if (requestAlgorithmMember.isOptional) {
                codegenContext.model.expectShape(requestAlgorithmMember.target)
            } else {
                requestAlgorithmMember
            }

        val operationName = symbolProvider.toSymbol(operationShape).name
        val interceptorName = "${operationName}HttpRequestChecksumMutationInterceptor"
        val requestChecksumRequired = checksumTrait.isRequestChecksumRequired

        writer.rustTemplate(
            """
            ##[derive(Debug)]
            struct $interceptorName;

            impl #{Intercept} for $interceptorName {
                fn name(&self) -> &'static str {
                    ${interceptorName.dq()}
                }

                fn modify_before_serialization(
                    &self,
                    context: &mut #{BeforeSerializationInterceptorContextMut}<'_, #{Input}, #{Output}, #{Error}>,
                    _runtime_comps: &#{RuntimeComponents},
                    cfg: &mut #{ConfigBag},
                ) -> #{Result}<(), #{BoxError}> {
                    let input = context
                        .input_mut()
                        .downcast_mut::<#{OperationInputType}>()
                        .ok_or("failed to downcast to #{OperationInputType}")?;

                    // This value is set by the user on the SdkConfig to indicate their preference
                    let request_checksum_calculation = cfg
                        .load::<#{RequestChecksumCalculation}>()
                        .unwrap_or(&#{RequestChecksumCalculation}::WhenSupported);

                    // From the httpChecksum trait
                    let http_checksum_required = $requestChecksumRequired;

                    // If the RequestChecksumCalculation is WhenSupported and the user has not set a checksum we
                    // default to Crc32. If it is WhenRequired and a checksum is required by the trait we also set the
                    // default. In all other cases we do nothing.
                    match (
                        request_checksum_calculation,
                        http_checksum_required,
                        input.checksum_algorithm(),
                    ) {
                        (#{RequestChecksumCalculation}::WhenSupported, _, None)
                        | (#{RequestChecksumCalculation}::WhenRequired, true, None) => {
                            input.checksum_algorithm = Some(#{ChecksumAlgoShape}::Crc32);
                        }
                        _ => {},
                    }

                    #{Ok}(())
                }
            }
            """,
            *codegenScope,
            "OperationInputType" to codegenContext.symbolProvider.toSymbol(operationShape.inputShape(model)),
            "ChecksumAlgoShape" to
                codegenContext.symbolProvider.toSymbol(
                    requestAlgorithmMemberInner,
                ),
        )
    }
}

/**
 * Get the top-level operation input member used to opt-in to best-effort validation of a checksum returned in
 * the HTTP response of the operation.
 */
fun HttpChecksumTrait.requestAlgorithmMember(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): MemberShape? {
    val requestAlgorithmMember = this.requestAlgorithmMember.orNull() ?: return null
    return operationShape.inputShape(codegenContext.model).expectMember(requestAlgorithmMember)
}
