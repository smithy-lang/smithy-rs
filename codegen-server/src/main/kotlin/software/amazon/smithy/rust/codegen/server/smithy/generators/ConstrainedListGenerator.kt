/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.ListShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape

// TODO Docs
// TODO Unit tests
class ConstrainedListGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val constrainedShapeSymbolProvider: ConstrainedShapeSymbolProvider,
    val writer: RustWriter,
    val shape: ListShape
) {
    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val symbol = symbolProvider.toSymbol(shape)
        val constrainedSymbol = constrainedShapeSymbolProvider.toSymbol(shape)
        // TODO Perhaps it's overkill to create this module when the type is pub.
        val module = constrainedSymbol.namespace.split(constrainedSymbol.namespaceDelimiter).last()
        val name = constrainedSymbol.name
        val innerShape = model.expectShape(shape.member.target)
        val innerConstrainedSymbol = constrainedShapeSymbolProvider.toSymbol(innerShape)

        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) Vec<#{InnerConstrainedSymbol}>);
                
                impl From<#{Symbol}> for $name {
                    fn from(v: #{Symbol}) -> Self {
                        Self(v.into_iter().map(|item| item.into()).collect())
                    }
                }

                impl From<$name> for #{Symbol} {
                    fn from(v: $name) -> Self {
                        v.0.into_iter().map(|item| item.into()).collect()
                    }
                }
                """,
                "InnerConstrainedSymbol" to innerConstrainedSymbol,
                "Symbol" to symbol,
            )
        }
    }
}
