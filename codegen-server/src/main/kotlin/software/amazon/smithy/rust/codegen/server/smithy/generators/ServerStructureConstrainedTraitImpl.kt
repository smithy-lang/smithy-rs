/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol

class ServerStructureConstrainedTraitImpl(
    private val symbolProvider: RustSymbolProvider,
    private val shape: StructureShape,
    private val writer: RustWriter
) {
    fun render() {
        writer.rustTemplate(
            """
            impl #{ConstrainedTrait} for #{Structure} {
                type Unconstrained = #{Builder};
            }
            """,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
            "Structure" to symbolProvider.toSymbol(shape),
            "Builder" to shape.builderSymbol(symbolProvider)
        )
    }
}
