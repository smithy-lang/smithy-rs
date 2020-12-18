/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.smithy

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.shapes.MemberShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.traits.IdempotencyTokenTrait

class IdempotencyTokenSymbolProvider(private val base: RustSymbolProvider) : WrappingSymbolProvider(base) {
    override fun toSymbol(shape: Shape): Symbol {
        val initial = base.toSymbol(shape)
        if (!shape.hasTrait(IdempotencyTokenTrait::class.java)) {
            return initial
        }
        check(shape is MemberShape)
        return initial.toBuilder().setDefault(
            Default.Custom {
                write("_config.token_provider.token()")
            }
        ).build()
    }
}
