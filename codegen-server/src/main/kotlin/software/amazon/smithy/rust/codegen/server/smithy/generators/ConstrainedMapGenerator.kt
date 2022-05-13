/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.Shape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.model.shapes.StructureShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.ConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained

// TODO Docs
// TODO Unit tests
class ConstrainedMapGenerator(
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val constrainedShapeSymbolProvider: ConstrainedShapeSymbolProvider,
    val writer: RustWriter,
    val shape: MapShape
) {
    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val symbol = symbolProvider.toSymbol(shape)
        val unconstrainedSymbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val constrainedSymbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val module = constrainedSymbol.namespace.split(constrainedSymbol.namespaceDelimiter).last()
        val name = constrainedSymbol.name
        val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
        val valueShape = model.expectShape(shape.value.target)
        val keySymbol = if (isKeyConstrained(keyShape)) {
            constrainedShapeSymbolProvider.toSymbol(keyShape)
        } else {
            symbolProvider.toSymbol(keyShape)
        }
        val valueSymbol = if (isValueConstrained(valueShape)) {
            constrainedShapeSymbolProvider.toSymbol(valueShape)
        } else {
            symbolProvider.toSymbol(valueShape)
        }

        // Unless the map holds an aggregate shape as its value shape whose symbol's type is _not_ `pub(crate)`, the
        // `.into()` calls are useless.
        // See the comment in [ConstrainedCollectionShape] for a more detailed explanation.
        val innerNeedsConstraining =
            !valueShape.isDirectlyConstrained(symbolProvider) && (valueShape is CollectionShape || valueShape.isMapShape)

        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>);
                
                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                
                impl From<#{Symbol}> for $name {
                    fn from(v: #{Symbol}) -> Self {
                        ${ if (innerNeedsConstraining) {
                            "Self(v.into_iter().map(|(k, v)| (k, v.into())).collect())"
                        } else {
                            "Self(v)"
                        } }
                    }
                }

                impl From<$name> for #{Symbol} {
                    fn from(v: $name) -> Self {
                        ${ if (innerNeedsConstraining) {
                            "v.0.into_iter().map(|(k, v)| (k, v.into())).collect()"
                        } else {
                            "v.0"
                        } }
                    }
                }
                """,
                "KeySymbol" to keySymbol,
                "ValueSymbol" to valueSymbol,
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
                "UnconstrainedSymbol" to unconstrainedSymbol,
                "Symbol" to symbol,
            )
        }
    }

    // TODO These are copied from `UnconstrainedMapGenerator.kt`.
    private fun isKeyConstrained(shape: StringShape) = shape.isDirectlyConstrained(symbolProvider)

    private fun isValueConstrained(shape: Shape): Boolean = when (shape) {
        is StructureShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is CollectionShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is MapShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is StringShape -> shape.isDirectlyConstrained(symbolProvider)
        // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Other constraint traits on simple shapes.
        else -> false
    }
}
