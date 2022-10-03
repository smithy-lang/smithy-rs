/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.traits.EnumTrait
import software.amazon.smithy.rust.codegen.client.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.client.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.client.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.client.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.core.util.expectTrait
import software.amazon.smithy.rust.codegen.server.smithy.ServerRuntimeType
import software.amazon.smithy.rust.codegen.server.smithy.makeMaybeConstrained

/**
 * [ConstrainedTraitForEnumGenerator] generates code that implements the [RuntimeType.ConstrainedTrait] trait on an
 * enum shape.
 */
class ConstrainedTraitForEnumGenerator(
    val model: Model,
    val codegenContext: ServerCodegenContext,
    val writer: RustWriter,
    val shape: StringShape,
) {
    fun render() {
        shape.expectTrait<EnumTrait>()

        val symbol = codegenContext.symbolProvider.toSymbol(shape)
        val name = symbol.name
        val unconstrainedType = "String"

        writer.rustTemplate(
            """
            impl #{ConstrainedTrait} for $name  {
                type Unconstrained = $unconstrainedType;
            }
            
            impl From<$unconstrainedType> for #{MaybeConstrained} {
                fn from(value: $unconstrainedType) -> Self {
                    Self::Unconstrained(value)
                }
            }
            """,
            "ConstrainedTrait" to ServerRuntimeType.ConstrainedTrait(codegenContext.runtimeConfig),
            "MaybeConstrained" to symbol.makeMaybeConstrained(),
        )
    }
}
