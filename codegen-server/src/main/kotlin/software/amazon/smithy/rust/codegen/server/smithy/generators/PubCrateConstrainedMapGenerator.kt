/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.model.shapes.StringShape
import software.amazon.smithy.rust.codegen.core.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.module
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.isTransitivelyButNotDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.typeNameContainsNonPublicType

/**
 * A generator for a wrapper tuple newtype over a map shape's symbol type.
 *
 * This newtype is for a map shape that is _transitively_ constrained, but not
 * directly. That is, the map shape does not have a constraint trait attached,
 * but the keys and/or values it holds reach a constrained shape. The generated
 * newtype is therefore `pub(crate)`, as the class name indicates, and is not
 * available to end users. After deserialization, upon constraint traits'
 * enforcement, this type is converted into the regular `HashMap` the user sees
 * via the generated converters.
 *
 * If the map shape is _directly_ constrained, use [ConstrainedMapGenerator]
 * instead.
 */
class PubCrateConstrainedMapGenerator(
    val codegenContext: ServerCodegenContext,
    val writer: RustWriter,
    val shape: MapShape,
) {
    private val model = codegenContext.model
    private val publicConstrainedTypes = codegenContext.settings.codegenConfig.publicConstrainedTypes
    private val unconstrainedShapeSymbolProvider = codegenContext.unconstrainedShapeSymbolProvider
    private val pubCrateConstrainedShapeSymbolProvider = codegenContext.pubCrateConstrainedShapeSymbolProvider
    private val constrainedShapeSymbolProvider = codegenContext.constrainedShapeSymbolProvider
    private val symbolProvider = codegenContext.symbolProvider

    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val symbol = symbolProvider.toSymbol(shape)
        val unconstrainedSymbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val constrainedSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)
        val name = constrainedSymbol.name
        val keyShape = model.expectShape(shape.key.target, StringShape::class.java)
        val valueShape = model.expectShape(shape.value.target)
        val keySymbol = constrainedShapeSymbolProvider.toSymbol(keyShape)
        val valueSymbol = if (valueShape.isTransitivelyButNotDirectlyConstrained(model, symbolProvider)) {
            pubCrateConstrainedShapeSymbolProvider.toSymbol(valueShape)
        } else {
            constrainedShapeSymbolProvider.toSymbol(valueShape)
        }

        val codegenScope = arrayOf(
            "KeySymbol" to keySymbol,
            "ValueSymbol" to valueSymbol,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait,
            "UnconstrainedSymbol" to unconstrainedSymbol,
            "Symbol" to symbol,
            "From" to RuntimeType.From,
        )

        writer.withInlineModule(constrainedSymbol.module()) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::collections::HashMap<#{KeySymbol}, #{ValueSymbol}>);

                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                """,
                *codegenScope,
            )

            if (publicConstrainedTypes) {
                // Unless the map holds an aggregate shape as its value shape whose symbol's type is _not_ `pub(crate)`, the
                // `.into()` calls are useless.
                // See the comment in [ConstrainedCollectionShape] for a more detailed explanation.
                val innerNeedsConstraining =
                    !valueShape.isDirectlyConstrained(symbolProvider) && (valueShape is CollectionShape || valueShape is MapShape)

                rustTemplate(
                    """
                    impl #{From}<#{Symbol}> for $name {
                        fn from(v: #{Symbol}) -> Self {
                            ${ if (innerNeedsConstraining) {
                        "Self(v.into_iter().map(|(k, v)| (k, v.into())).collect())"
                    } else {
                        "Self(v)"
                    } }
                        }
                    }

                    impl #{From}<$name> for #{Symbol} {
                        fn from(v: $name) -> Self {
                            ${ if (innerNeedsConstraining) {
                        "v.0.into_iter().map(|(k, v)| (k, v.into())).collect()"
                    } else {
                        "v.0"
                    } }
                        }
                    }
                    """,
                    *codegenScope,
                )
            } else {
                val keyNeedsConversion = keyShape.typeNameContainsNonPublicType(model, symbolProvider, publicConstrainedTypes)
                val valueNeedsConversion = valueShape.typeNameContainsNonPublicType(model, symbolProvider, publicConstrainedTypes)

                rustTemplate(
                    """
                    impl #{From}<$name> for #{Symbol} {
                        fn from(v: $name) -> Self {
                            ${ if (keyNeedsConversion || valueNeedsConversion) {
                        val keyConversion = if (keyNeedsConversion) { ".into()" } else { "" }
                        val valueConversion = if (valueNeedsConversion) { ".into()" } else { "" }
                        "v.0.into_iter().map(|(k, v)| (k$keyConversion, v$valueConversion)).collect()"
                    } else {
                        "v.0"
                    } }
                        }
                    }
                    """,
                    *codegenScope,
                )
            }
        }
    }
}
