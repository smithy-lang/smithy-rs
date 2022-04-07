/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider

class ConstrainedListGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    val writer: RustWriter,
    val shape: ListShape
) {
    private val symbol = symbolProvider.toSymbol(shape)
    private val name = "${shape.id.name}Wrapper"

    fun render() {
        // TODO NO! Implement it for the vector type directly, no need for wrapper type.
        // TODO Unit test that this is pub(crate).
        writer.rustTemplate(
            """
            pub(crate) struct $name(#{UnvalidatedList});
            
            impl #{ValidateTrait} for $name {
                type Unvalidated = #{UnvalidatedList};
            }
            """,
            "ValidateTrait" to RuntimeType.ValidateTrait(),
            // TODO Note that when we have constraint shapes, this symbol will be incorrect; we need the corresponding
            //  "unconstrained" symbol.
            "UnvalidatedList" to symbol,
        )
    }
}
