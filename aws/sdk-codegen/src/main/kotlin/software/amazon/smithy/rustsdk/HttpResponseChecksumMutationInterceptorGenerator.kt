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
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.util.dq
import software.amazon.smithy.rust.codegen.core.util.expectMember
import software.amazon.smithy.rust.codegen.core.util.getTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape
import software.amazon.smithy.rust.codegen.core.util.orNull

/**
 * This class generates an interceptor for operations with the `httpChecksum` trait that support response validations.
 * In the `modify_before_serialization` hook the interceptor checks the operation's `requestValidationModeMember`. If
 * that member is `ENABLED` then we end early and do nothing. If it is not `ENABLED` then it checks the
 * `response_checksum_validation` set by the user on the SdkConfig. If that is `WhenSupported` (or unknown) then we
 * update the `requestValidationModeMember` to `ENABLED` and if the value is `WhenRequired` we end without modifying
 * anything since there is no way to indicate that a response checksum is required.
 *
 * Note that although there is an existing inlineable `ResponseChecksumInterceptor` this logic could not live there.
 * Since that interceptor is inlineable it does not have access to the name of the `requestValidationModeMember` on the
 * operation's input, and in certain circumstances we need to mutate that member on the input before serializing the
 * request and sending it to the service.
 */
class HttpResponseChecksumMutationInterceptorGenerator : ClientCodegenDecorator {
    override val name: String = "HttpResponseChecksumMutationInterceptorGenerator"
    override val order: Byte = 0

    override fun operationCustomizations(
        codegenContext: ClientCodegenContext,
        operation: OperationShape,
        baseCustomizations: List<OperationCustomization>,
    ): List<OperationCustomization> {
        // If the operation doesn't have the HttpChecksum trait we return early
        val checksumTrait = operation.getTrait<HttpChecksumTrait>() ?: return baseCustomizations
        // Also return early if there is no requestValidationModeMember on the trait
        val requestValidationModeMember =
            (checksumTrait.requestValidationModeMemberShape(codegenContext, operation) ?: return baseCustomizations)

        return baseCustomizations +
            listOf(
                InterceptorSection(
                    codegenContext,
                    operation,
                    requestValidationModeMember,
                    checksumTrait,
                ),
            )
    }

    private class InterceptorSection(
        private val codegenContext: ClientCodegenContext,
        private val operation: OperationShape,
        private val requestValidationModeMember: MemberShape,
        private val checksumTrait: HttpChecksumTrait,
    ) : OperationCustomization() {
        override fun section(section: OperationSection): Writable =
            writable {
                if (section is OperationSection.RuntimePluginSupportingTypes) {
                    val model = codegenContext.model
                    val symbolProvider = codegenContext.symbolProvider
                    val codegenScope =
                        codegenContext.runtimeConfig.let { rc ->
                            val runtimeApi = CargoDependency.smithyRuntimeApiClient(rc).toType()
                            val interceptors = runtimeApi.resolve("client::interceptors")
                            val orchestrator = runtimeApi.resolve("client::orchestrator")

                            arrayOf(
                                *preludeScope,
                                "BoxError" to RuntimeType.boxError(rc),
                                "ConfigBag" to RuntimeType.configBag(rc),
                                "Intercept" to RuntimeType.intercept(rc),
                                "BeforeSerializationInterceptorContextMut" to
                                    RuntimeType.beforeSerializationInterceptorContextMut(
                                        rc,
                                    ),
                                "Input" to interceptors.resolve("context::Input"),
                                "Output" to interceptors.resolve("context::Output"),
                                "Error" to interceptors.resolve("context::Error"),
                                "RuntimeComponents" to RuntimeType.runtimeComponents(rc),
                                "ResponseChecksumValidation" to
                                    CargoDependency.smithyChecksums(rc).toType()
                                        .resolve("ResponseChecksumValidation"),
                            )
                        }

                    val requestValidationModeName = symbolProvider.toSymbol(requestValidationModeMember).name
                    val requestValidationModeMemberInner =
                        if (requestValidationModeMember.isOptional) {
                            codegenContext.model.expectShape(requestValidationModeMember.target)
                        } else {
                            requestValidationModeMember
                        }

                    val operationName = symbolProvider.toSymbol(operation).name
                    val interceptorName = "${operationName}HttpResponseChecksumMutationInterceptor"

                    rustTemplate(
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

                                    // If validation setting is WhenSupported (or unknown) we enable response checksum
                                    // validation. If it is WhenRequired we do not enable (since there is no way to
                                    // indicate that a response checksum is required).
                                    ##[allow(clippy::wildcard_in_or_patterns)]
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
                        "OperationInputType" to codegenContext.symbolProvider.toSymbol(operation.inputShape(model)),
                        "ValidationModeShape" to
                            codegenContext.symbolProvider.toSymbol(
                                requestValidationModeMemberInner,
                            ),
                    )
                }
            }
    }
}

/**
 * Get the top-level operation input member used to opt in to best-effort validation of a checksum returned in
 * the HTTP response of the operation.
 */
fun HttpChecksumTrait.requestValidationModeMemberShape(
    codegenContext: ClientCodegenContext,
    operationShape: OperationShape,
): MemberShape? {
    val requestValidationModeMember = this.requestValidationModeMember.orNull() ?: return null
    return operationShape.inputShape(codegenContext.model).expectMember(requestValidationModeMember)
}
