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
import software.amazon.smithy.rust.codegen.smithy.isConstrained

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

        // The converters are only needed when the constrained type is `pub(crate)`, for the server builder function
        // member function to work.
        // Note that unless the map holds an aggregate shape as its value shape, the `.into()` calls are useless.
        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>);
                
                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                
                impl From<#{Symbol}> for $name {
                    fn from(hm: #{Symbol}) -> Self {
                        Self(hm.into_iter().map(|(k, v)| (k, v.into())).collect())
                    }
                }

                impl From<$name> for #{Symbol} {
                    fn from(wrapper: $name) -> Self {
                        wrapper.0.into_iter().map(|(k, v)| (k, v.into())).collect()
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
    private fun isKeyConstrained(shape: StringShape) = shape.isConstrained(symbolProvider)

    private fun isValueConstrained(shape: Shape): Boolean = when (shape) {
        is StructureShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is CollectionShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is MapShape -> shape.canReachConstrainedShape(model, symbolProvider)
        is StringShape -> shape.isConstrained(symbolProvider)
        // TODO(https://github.com/awslabs/smithy-rs/pull/1199) Other constraint traits on simple shapes.
        else -> false
    }
}
