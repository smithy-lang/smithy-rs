/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.rust.codegen.client.smithy.ClientRustModule
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationCustomization
import software.amazon.smithy.rust.codegen.client.smithy.generators.OperationSection
import software.amazon.smithy.rust.codegen.core.rustlang.CargoDependency
import software.amazon.smithy.rust.codegen.core.rustlang.InlineDependency
import software.amazon.smithy.rust.codegen.core.rustlang.RustModule
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.toType
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.core.util.findMemberWithTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape

class IdempotencyTokenGenerator(
    codegenContext: CodegenContext,
    operationShape: OperationShape,
) : OperationCustomization() {
    private val model = codegenContext.model
    private val runtimeConfig = codegenContext.runtimeConfig
    private val symbolProvider = codegenContext.symbolProvider
    private val inputShape = operationShape.inputShape(model)
    private val idempotencyTokenMember = inputShape.findMemberWithTrait<IdempotencyTokenTrait>(model)

    override fun section(section: OperationSection): Writable {
        if (idempotencyTokenMember == null) {
            return emptySection
        }
        val memberName = symbolProvider.toMemberName(idempotencyTokenMember)
        val codegenScope = arrayOf(
            *preludeScope,
            "Input" to symbolProvider.toSymbol(inputShape),
            "IdempotencyTokenRuntimePlugin" to
                InlineDependency.forRustFile(
                    RustModule.pubCrate("client_idempotency_token", parent = ClientRustModule.root),
                    "/inlineable/src/client_idempotency_token.rs",
                    CargoDependency.smithyRuntimeApi(runtimeConfig),
                    CargoDependency.smithyTypes(runtimeConfig),
                ).toType().resolve("IdempotencyTokenRuntimePlugin"),
            "SharedRuntimePlugin" to RuntimeType.sharedRuntimePlugin(runtimeConfig),
        )

        return when (section) {
            is OperationSection.AdditionalRuntimePlugins -> writable {
                section.addOperationRuntimePlugin(this) {
                    if (symbolProvider.toSymbol(idempotencyTokenMember).isOptional()) {
                        // An idempotency token is optional. If the user didn't specify a token
                        // then we'll generate one and set it.
                        rustTemplate(
                            """
                            #{SharedRuntimePlugin}::new(
                                #{IdempotencyTokenRuntimePlugin}::new(|token_provider, input| {
                                    let input: &mut #{Input} = input.downcast_mut().expect("correct type");
                                    if input.$memberName.is_none() {
                                        input.$memberName = #{Some}(token_provider.make_idempotency_token());
                                    }
                                })
                            )
                            """,
                            *codegenScope,
                        )
                    } else {
                        // An idempotency token is required, but it'll be set to an empty string if
                        // the user didn't specify one. If that's the case, then we'll generate one
                        // and set it.
                        rustTemplate(
                            """
                            #{SharedRuntimePlugin}::new(
                                #{IdempotencyTokenRuntimePlugin}::new(|token_provider, input| {
                                    let input: &mut #{Input} = input.downcast_mut().expect("correct type");
                                    if input.$memberName.is_empty() {
                                        input.$memberName = token_provider.make_idempotency_token();
                                    }
                                })
                            )
                            """,
                            *codegenScope,
                        )
                    }
                }
            }
            else -> emptySection
        }
    }
}
