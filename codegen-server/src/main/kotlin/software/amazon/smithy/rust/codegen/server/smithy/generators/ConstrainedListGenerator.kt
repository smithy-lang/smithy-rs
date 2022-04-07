/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.mapRustType
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.shape

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

        val unvalidatedSymbol = mapStructToBuilderRecursively(symbol)

        writer.rustTemplate(
            """
            impl #{ValidateTrait} for #{Shape}  {
                type Unvalidated = #{UnvalidatedSymbol};
            }
            
            impl std::convert::TryFrom<Builder> for #{Structure} {
                type Error = ValidationFailure;
                
                fn try_from(builder: Builder) -> Result<Self, Self::Error> {
                    builder.build()
                }
            }
            """,
            "ValidateTrait" to RuntimeType.ValidateTrait(),
            // TODO Note that when we have constraint shapes, this symbol will be incorrect; we need the corresponding
            //  "unconstrained" symbol.
            "Shape" to symbol,
            "UnvalidatedSymbol" to unvalidatedSymbol,
        )
    }

    private fun mapStructToBuilderRecursively(symbol: Symbol): Symbol {
        // TODO I think this will fail for lists of hash maps.
        check(symbol.references.size <= 1)

        val shape = symbol.shape()

        if (shape.isStructureShape) {
            val builderSymbolBuilder = shape.asStructureShape().get().builderSymbol(symbolProvider).toSymbol().toBuilder()
            check(symbol.references.isEmpty())
            return builderSymbolBuilder.build()
        } else {
            return if (symbol.references.isEmpty()) {
                symbol
            } else {
                // TODO This will fail with maps
                check(shape.isListShape)

                val newSymbol = mapStructToBuilderRecursively(symbol.references[0].symbol)
                val newType = RustType.Vec(newSymbol.rustType())
                // TODO Not using `mapRustType` because it adds a reference to itself, which I don't udnerstand.
                Symbol.builder()
                    .rustType(newType)
                    .addReference(newSymbol)
                    .name(newType.name)
                    .build()
            }
        }
    }
}
