/*
 * Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
 * SPDX-License-Identifier: Apache-2.0.
 */

package software.amazon.smithy.rust.codegen.server.smithy.generators

import software.amazon.smithy.model.Model
import software.amazon.smithy.model.shapes.CollectionShape
import software.amazon.smithy.rust.codegen.rustlang.RustMetadata
import software.amazon.smithy.rust.codegen.rustlang.RustWriter
import software.amazon.smithy.rust.codegen.rustlang.Visibility
import software.amazon.smithy.rust.codegen.rustlang.rustTemplate
import software.amazon.smithy.rust.codegen.smithy.PubCrateConstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.RuntimeType
import software.amazon.smithy.rust.codegen.smithy.RustSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.UnconstrainedShapeSymbolProvider
import software.amazon.smithy.rust.codegen.smithy.canReachConstrainedShape
import software.amazon.smithy.rust.codegen.smithy.isDirectlyConstrained
import software.amazon.smithy.rust.codegen.smithy.isTransitivelyConstrained

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
    val model: Model,
    val symbolProvider: RustSymbolProvider,
    private val unconstrainedShapeSymbolProvider: UnconstrainedShapeSymbolProvider,
    private val pubCrateConstrainedShapeSymbolProvider: PubCrateConstrainedShapeSymbolProvider,
    private val constrainedShapeSymbolProvider: RustSymbolProvider,
    private val publicConstrainedTypes: Boolean,
    val writer: RustWriter,
    val shape: CollectionShape
) {
    fun render() {
        check(shape.canReachConstrainedShape(model, symbolProvider))

        val constrainedSymbol = constrainedShapeSymbolProvider.toSymbol(shape)
        val pubCrateConstrainedSymbol = pubCrateConstrainedShapeSymbolProvider.toSymbol(shape)
        val unconstrainedSymbol = unconstrainedShapeSymbolProvider.toSymbol(shape)
        val module = pubCrateConstrainedSymbol.namespace.split(pubCrateConstrainedSymbol.namespaceDelimiter).last()
        val name = pubCrateConstrainedSymbol.name
        val innerShape = model.expectShape(shape.member.target)
        val innerConstrainedSymbol = if (innerShape.isTransitivelyConstrained(model, symbolProvider)) {
            pubCrateConstrainedShapeSymbolProvider.toSymbol(innerShape)
        } else {
            constrainedShapeSymbolProvider.toSymbol(innerShape)
        }

        // If the target member shape is itself _not_ directly constrained, and is an aggregate non-Structure shape,
        // then its corresponding constrained type is the `pub(crate)` wrapper tuple type, which needs converting into
        // the public type the user is exposed to. The two types are isomorphic, and we can convert between them using
        // `From`. So we track this particular case here in order to iterate over the list's members and convert
        // each of them.
        //
        // Note that we could add the iteration code unconditionally and it would still be correct, but the `into()` calls
        // would be useless. Clippy flags this as [`useless_conversion`]. We could deactivate the lint, but it's probably
        // best that we just don't emit a useless iteration, lest the compiler not optimize it away, and to make the
        // generated code a little bit simpler.
        //
        // [`useless_conversion`]: https://rust-lang.github.io/rust-clippy/master/index.html#useless_conversion.
        val innerNeedsConstraining =
            !innerShape.isDirectlyConstrained(symbolProvider) && (innerShape is CollectionShape || innerShape.isMapShape)

        writer.withModule(module, RustMetadata(visibility = Visibility.PUBCRATE)) {
            rustTemplate(
                """
                ##[derive(Debug, Clone)]
                pub(crate) struct $name(pub(crate) std::vec::Vec<#{InnerConstrainedSymbol}>);

                impl #{ConstrainedTrait} for $name  {
                    type Unconstrained = #{UnconstrainedSymbol};
                }

                impl #{From}<#{ConstrainedSymbol}> for $name {
                    fn from(v: #{ConstrainedSymbol}) -> Self {
                        ${ if (innerNeedsConstraining) {
                            "Self(v.into_iter().map(|item| item.into()).collect())"
                        } else {
                            "Self(v)"
                        } }
                    }
                }

                impl #{From}<$name> for #{ConstrainedSymbol} {
                    fn from(v: $name) -> Self {
                        ${ if (innerNeedsConstraining) {
                            "v.0.into_iter().map(|item| item.into()).collect()"
                        } else {
                            "v.0"
                        } }
                    }
                }
                """,
                "From" to RuntimeType.From,
                "InnerConstrainedSymbol" to innerConstrainedSymbol,
                "ConstrainedTrait" to RuntimeType.ConstrainedTrait(),
                "UnconstrainedSymbol" to unconstrainedSymbol,
                "ConstrainedSymbol" to constrainedSymbol,
            )

//            if (!publicConstrainedTypes) {
//                // Note that if public constrained types is not enabled, then the regular `symbolProvider` produces
//                // "fully unconstrained" symbols for all shapes (i.e. as if the shapes didn't have any constraint traits).
//                rustTemplate(
//                    """
//                    impl #{From}<$name> for #{FullyUnconstrainedSymbol} {
//                        fn from(v: $name) -> Self {
//                            v.0.into_iter().map(|item| item.into()).collect()
//                        }
//                    }
//                    """,
//                    "From" to RuntimeType.From,
//                    "FullyUnconstrainedSymbol" to symbolProvider.toSymbol(shape),
//                )
//            }
        }
    }
}
