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
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType.Companion.preludeScope
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
        return when (section) {
            is OperationSection.AdditionalRuntimePlugins -> writable {
                section.addOperationRuntimePlugin(this) {
                    rustTemplate(
                        """
                        #{IdempotencyTokenRuntimePlugin}::new(|token_provider, input| {
                            let input: &mut #{Input} = input.downcast_mut().expect("correct type");
                            if input.$memberName.is_none() {
                                input.$memberName = #{Some}(token_provider.make_idempotency_token());
                            }
                        })
                        """,
                        *preludeScope,
                        "Input" to symbolProvider.toSymbol(inputShape),
                        "IdempotencyTokenRuntimePlugin" to
                            InlineDependency.forRustFile(
                                RustModule.pubCrate("client_idempotency_token", parent = ClientRustModule.root),
                                "/inlineable/src/client_idempotency_token.rs",
                                CargoDependency.smithyRuntimeApi(runtimeConfig),
                                CargoDependency.smithyTypes(runtimeConfig),
                            ).toType().resolve("IdempotencyTokenRuntimePlugin"),
                    )
                }
            }
            is OperationSection.MutateInput -> writable {
                rustTemplate(
                    """
                    if ${section.input}.$memberName.is_none() {
                        ${section.input}.$memberName = #{Some}(${section.config}.idempotency_token_provider.make_idempotency_token());
                    }
                    """,
                    *preludeScope,
                )
            }
            else -> emptySection
        }
    }

    override fun consumesSelf(): Boolean = idempotencyTokenMember != null
    override fun mutSelf(): Boolean = idempotencyTokenMember != null
}
