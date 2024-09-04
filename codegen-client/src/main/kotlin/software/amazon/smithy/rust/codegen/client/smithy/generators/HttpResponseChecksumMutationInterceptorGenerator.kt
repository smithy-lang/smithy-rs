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
 * This class generates a interceptor for operations with the `httpChecksum` trait that support response validations.
 * In the `modify_before_serialization` hook the interceptor checks the operation's `requestValidationModeMember`. If
 * that member is `ENABLED` then we end early and do nothing. If it is not `ENABLED` then it checks the
 * `response_checksum_validation` set by the user on the SdkConfig. If that is `WhenSupported` (or unknown) then we
 * update the `requestValidationModeMember` to `ENABLED` and if the value is `WhenRequired` we end without modifying
 * anything.
 *
 * Note that although there is an existing inlineable `ResponseChecksumInterceptor` this logic could not live there.
 * Since that interceptor is inlineable it does not have access to the name of the `requestValidationModeMember` on the
 * operation's input, and in certain circumstances we need to mutate that member on the input before serializing the
 * request and sending it to the service.
 */
class HttpResponseChecksumMutationInterceptorGenerator(
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
                "ResponseChecksumValidation" to
                    CargoDependency.smithyChecksums(rc).toType()
                        .resolve("ResponseChecksumValidation"),
            )
        }

    fun render(
        writer: RustWriter,
        operationShape: OperationShape,
    ) {
        // If the operation doesn't have the HttpChecksum trait we return early
        val checksumTrait = operationShape.getTrait<HttpChecksumTrait>() ?: return
        // Also return early if there is no requestValidationModeMember on the trait
        val requestValidationModeMember =
            (checksumTrait.requestValidationModeMember(codegenContext, operationShape) ?: return)
        val requestValidationModeName = symbolProvider.toSymbol(requestValidationModeMember).name
        val requestValidationModeMemberInner =
            if (requestValidationModeMember.isOptional) {
                codegenContext.model.expectShape(requestValidationModeMember.target)
            } else {
                requestValidationModeMember
            }

        val operationName = symbolProvider.toSymbol(operationShape).name
        val interceptorName = "${operationName}HttpResponseChecksumMutationInterceptor"

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

                    let request_validation_enabled =
                        matches!(input.$requestValidationModeName(), Some(#{ValidationModeShape}::Enabled));

                    if !request_validation_enabled {
                        // This value is set by the user on the SdkConfig to indicate their preference
                        let response_checksum_validation = cfg
                            .load::<#{ResponseChecksumValidation}>()
                            .unwrap_or(&#{ResponseChecksumValidation}::WhenSupported);

                        match response_checksum_validation {
                            #{ResponseChecksumValidation}::WhenRequired => {}
                            #{ResponseChecksumValidation}::WhenSupported | _ => {
                                input.$requestValidationModeName = Some(#{ValidationModeShape}::Enabled);
                            }
                        }
                    }

                    #{Ok}(())
                }
            }
            """,
            *codegenScope,
            "OperationInputType" to codegenContext.symbolProvider.toSymbol(operationShape.inputShape(model)),
            "ValidationModeShape" to
                codegenContext.symbolProvider.toSymbol(
                    requestValidationModeMemberInner,
                ),
        )
    }
}

/**
 * Get the top-level operation input member used to opt-in to best-effort validation of a checksum returned in
 * the HTTP response of the operation.
 */
fun HttpChecksumTrait.requestValidationModeMember(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): MemberShape? {
    val requestValidationModeMember = this.requestValidationModeMember.orNull() ?: return null
    return operationShape.inputShape(codegenContext.model).expectMember(requestValidationModeMember)
}
