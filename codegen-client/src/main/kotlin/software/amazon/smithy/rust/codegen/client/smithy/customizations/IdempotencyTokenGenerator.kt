/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.client.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.rust.codegen.core.rustlang.Writable
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.writable
import software.amazon.smithy.rust.codegen.core.smithy.CodegenContext
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.core.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.core.util.findMemberWithTrait
import software.amazon.smithy.rust.codegen.core.util.inputShape

class IdempotencyTokenGenerator(codegenContext: CodegenContext, operationShape: OperationShape) :
    OperationCustomization() {
    private val model = codegenContext.model
    private val symbolProvider = codegenContext.symbolProvider
    private val idempotencyTokenMember = operationShape.inputShape(model).findMemberWithTrait<IdempotencyTokenTrait>(model)
    override fun section(section: OperationSection): Writable {
        if (idempotencyTokenMember == null) {
            return emptySection
        }
        val memberName = symbolProvider.toMemberName(idempotencyTokenMember)
        return when (section) {
            is OperationSection.MutateInput -> writable {
                rust(
                    """
                    if ${section.input}.$memberName.is_none() {
                        ${section.input}.$memberName = Some(${section.config}.make_token.make_idempotency_token());
                    }
                    """,
                )
            }
            else -> emptySection
        }
    }

    override fun consumesSelf(): Boolean = idempotencyTokenMember != null
    override fun mutSelf(): Boolean = idempotencyTokenMember != null
}
