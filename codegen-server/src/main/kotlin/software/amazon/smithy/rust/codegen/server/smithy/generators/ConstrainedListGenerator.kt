/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.codegen.core.Symbol
import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustType
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.server.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.generators.builderSymbol
import software.amazon.smithy.rust.codegen.smithy.rustType
import software.amazon.smithy.rust.codegen.smithy.shape
import software.amazon.smithy.rust.codegen.util.toSnakeCase

// TODO Rename to Unconstrained?
class ConstrainedListGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    val writer: RustWriter,
    val shape: ListShape
) {
    private val symbol = symbolProvider.toSymbol(shape)

    fun render() {
        check(shape.canReachConstrainedShape(model))

        // TODO Unit test that this is pub(crate).

        val symbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val module = symbol.namespace.split(symbol.namespaceDelimiter).last()
        val name = symbol.name
        val innerSymbol = unconstrainedShapeSymbolProvider.toSymbol(model.expectShape(shape.member.target))

        // impl #{ValidateTrait} for #{Shape}  {
        //     type Unvalidated = #{UnvalidatedSymbol};
        // }
        //
        // impl std::convert::TryFrom<Builder> for #{Structure} {
        //     type Error = ValidationFailure;
        //
        //     fn try_from(builder: Builder) -> Result<Self, Self::Error> {
        //         builder.build()
        //     }
        // }

        writer.withModule(module, RustMetadata(public = false)) {
            rustTemplate(
                """
                pub(crate) struct $name(#{UnconstrainedInnerSymbol});
                """,
//            "ValidateTrait" to RuntimeType.ValidateTrait(),
                "UnconstrainedInnerSymbol" to innerSymbol,
//            "Shape" to symbol,
//            "UnvalidatedSymbol" to unvalidatedSymbol,
            )
        }
    }

//    private fun unconstrainedSymbol(shape: ListShape): Symbol {
//        mapStructToBuilderRecursively(symbol)
//    }

//    private fun structDefinition() = if (shape.isConstrained()) {
//        "pub ${name()}();"
//    } else {
//        "${name()};"
//    }
//
//    private fun name() = if (shape.isConstrained()) {
//        shape.id.name
//    } else {
//        "${shape.id.name}Constrained"
//    }

    private fun mapStructToBuilderRecursively(symbol: Symbol): Symbol {
        // TODO I think this will fail for lists of hash maps.
        check(symbol.references.size <= 1)

        val shape = symbol.shape()

        if (shape.isStructureShape) {
            val builderSymbolBuilder = shape.asStructureShape().get().builderSymbol(symbolProvider).toBuilder()
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
                // TODO Not using `mapRustType` because it adds a reference to itself, which is wrong.
                Symbol.builder()
                    .rustType(newType)
                    .addReference(newSymbol)
                    .name(newType.name)
                    .build()
            }
        }
    }
}
