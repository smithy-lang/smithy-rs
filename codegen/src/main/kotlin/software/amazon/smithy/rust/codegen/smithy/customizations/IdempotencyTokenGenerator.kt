/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.smithy.customizations

import software.amazon.smithy.model.shapes.OperationShape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait
import software.amazon.smithy.rust.codegen.rustlang.Writable
import software.amazon.smithy.rust.codegen.rustlang.rust
import software.amazon.smithy.rust.codegen.rustlang.writable
import software.amazon.smithy.rust.codegen.smithy.CoreCodegenContext
import software.amazon.smithy.rust.codegen.smithy.customize.OperationCustomization
import software.amazon.smithy.rust.codegen.smithy.customize.OperationSection
import software.amazon.smithy.rust.codegen.util.findMemberWithTrait
import software.amazon.smithy.rust.codegen.util.inputShape

class IdempotencyTokenGenerator(coreCodegenContext: CoreCodegenContext, private val operationShape: OperationShape) :
    OperationCustomization() {
    private val model = coreCodegenContext.model
    private val symbolProvider = coreCodegenContext.symbolProvider
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
                    """
                )
            }
            else -> emptySection
        }
    }

    override fun consumesSelf(): Boolean = idempotencyTokenMember != null
    override fun mutSelf(): Boolean = idempotencyTokenMember != null
}
