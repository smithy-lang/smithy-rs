/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.model.shapes.MapShape
import software.amazon.smithy.rust.codegen.core.rustlang.conditionalBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rust
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlock
import software.amazon.smithy.rust.codegen.core.rustlang.rustBlockTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.core.rustlang.withBlock
import software.amazon.smithy.rust.codegen.core.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.core.smithy.isOptional
import software.amazon.smithy.rust.codegen.server.smithy.InlineModuleCreator
import software.amazon.smithy.rust.codegen.server.smithy.ServerCodegenContext
import software.amazon.smithy.rust.codegen.server.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.server.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.isTransitivelyButNotDirectlyConstrained
import software.amazon.smithy.rust.codegen.server.smithy.typeNameContainsNonPublicType

/**
 * A generator for a wrapper tuple newtype over a collection shape's symbol
 * type.
 *
 * This newtype is for a collection shape that is _transitively_ constrained,
 * but not directly. That is, the collection shape does not have a constraint
 * trait attached, but the members it holds reach a constrained shape. The
 * generated newtype is therefore `pub(crate)`, as the class name indicates,
 * and is not available to end users. After deserialization, upon constraint
 * traits' enforcement, this type is converted into the regular `Vec` the user
 * sees via the generated converters.
 *
 * TODO(https://github.com/awslabs/smithy-rs/issues/1401) If the collection
 *  shape is _directly_ constrained, use [ConstrainedCollectionGenerator]
 *  instead.
 */
class PubCrateConstrainedCollectionGenerator(
    val codegenContext: ServerCodegenContext,
    private val inlineModuleCreator: InlineModuleCreator,
    val shape: CollectionShape,
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
        val constrainedSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)

        val unconstrainedSymbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val name = constrainedSymbol.name
        val innerShape = model.expectShape(shape.member.target)
        val innerMemberSymbol = if (innerShape.isTransitivelyButNotDirectlyConstrained(model, symbolProvider)) {
            pubCrateConstrainedShapeSymbolProvider.toSymbol(shape.member)
        } else {
            constrainedShapeSymbolProvider.toSymbol(shape.member)
        }

        val codegenScope = arrayOf(
            "InnerMemberSymbol" to innerMemberSymbol,
            "ConstrainedTrait" to RuntimeType.ConstrainedTrait,
            "UnconstrainedSymbol" to unconstrainedSymbol,
            "Symbol" to symbol,
            "From" to RuntimeType.From,
        )

        inlineModuleCreator(constrainedSymbol) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::vec::Vec<#{InnerMemberSymbol}>);

                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }
                """,
                *codegenScope,
            )

            if (publicConstrainedTypes) {
                // If the target member shape is itself _not_ directly constrained, and is an aggregate non-Structure shape,
                // then its corresponding constrained type is the `pub(crate)` wrapper tuple type, which needs converting into
                // the public type the user is exposed to. The two types are isomorphic, and we can convert between them using
                // `From`. So we track this particular case here in order to iterate over the list's members and convert
                // each of them.
                //
                // Note that we could add the iteration code unconditionally and it would still be correct, but the `into()` calls
                // would be useless. Clippy flags this as [`useless_conversion`]. We could deactivate the lint, but it's probably
                // best that we just don't emit a useless iteration, lest the compiler not optimize it away (see [Godbolt]),
                // and to make the generated code a little bit simpler.
                //
                // [`useless_conversion`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_conversion.
                // [Godbolt]: https://godbolt.org/z/eheWebWMa
                val innerNeedsConstraining =
                    !innerShape.isDirectlyConstrained(symbolProvider) && (innerShape is CollectionShape || innerShape is MapShape)

                rustTemplate(
                    """
                    impl #{From}<#{Symbol}> for $name {
                        fn from(v: #{Symbol}) -> Self {
                            ${
                        if (innerNeedsConstraining) {
                            "Self(v.into_iter().map(|item| item.into()).collect())"
                        } else {
                            "Self(v)"
                        }
                    }
                        }
                    }

                    impl #{From}<$name> for #{Symbol} {
                        fn from(v: $name) -> Self {
                            ${
                        if (innerNeedsConstraining) {
                            "v.0.into_iter().map(|item| item.into()).collect()"
                        } else {
                            "v.0"
                        }
                    }
                        }
                    }
                    """,
                    *codegenScope,
                )
            } else {
                val innerNeedsConversion =
                    innerShape.typeNameContainsNonPublicType(model, symbolProvider, publicConstrainedTypes)

                rustBlockTemplate("impl #{From}<$name> for #{Symbol}", *codegenScope) {
                    rustBlock("fn from(v: $name) -> Self") {
                        if (innerNeedsConversion) {
                            withBlock("v.0.into_iter().map(|item| ", ").collect()") {
                                conditionalBlock("item.map(|item| ", ")", innerMemberSymbol.isOptional()) {
                                    rust("item.into()")
                                }
                            }
                        } else {
                            rust("v.0")
                        }
                    }
                }
            }
        }
    }
}
